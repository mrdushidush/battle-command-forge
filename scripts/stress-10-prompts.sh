#!/bin/bash
# Run the pipeline on 10 diverse prompts to benchmark the full system
# Usage: bash scripts/stress-10-prompts.sh
set -e

BINARY="./target/release/battlecommand-forge"
RESULTS_DIR="output/stress-test-$(date +%Y%m%d_%H%M%S)"
mkdir -p "$RESULTS_DIR"

PROMPTS=(
    "Build a Python FastAPI REST API with two endpoints: POST /users to create a user and GET /users/{id} to get a user. Use SQLite with SQLAlchemy async. Include pytest tests."
    "Build a Python CLI tool that converts CSV files to JSON. Support stdin/stdout piping. Include unit tests."
    "Build a Python FastAPI authentication service with JWT login, refresh tokens, password hashing, and role-based access control. Use PostgreSQL with SQLAlchemy async. Include comprehensive tests."
    "Build a Python websocket chat server using FastAPI. Support multiple rooms, user nicknames, message history stored in Redis. Include tests."
    "Build a Python REST API for a todo list app. CRUD endpoints for tasks with due dates, priorities, and tags. SQLite backend. Include tests."
    "Build a Python script that monitors a directory for new files and sends email notifications using SMTP. Include tests."
    "Build a Python FastAPI microservice for URL shortening. POST to create short URL, GET to redirect. Track click counts. Use SQLite. Include tests."
    "Build a Python library for validating and parsing configuration files (YAML, TOML, JSON). Support schema validation, defaults, and environment variable interpolation. Include tests."
    "Build a Python FastAPI e-commerce API with products, categories, shopping cart, and checkout. Use PostgreSQL with SQLAlchemy async. Include tests."
    "Build a simple Python HTTP health check service that pings a list of URLs and returns their status. Single file if possible. Include tests."
)

LABELS=(
    "C4_simple_crud"
    "C3_csv_cli"
    "C8_auth_rbac"
    "C7_websocket_chat"
    "C5_todo_crud"
    "C4_file_monitor"
    "C5_url_shortener"
    "C6_config_parser"
    "C9_ecommerce"
    "C2_health_check"
)

SUMMARY_FILE="$RESULTS_DIR/summary.txt"
CSV_FILE="$RESULTS_DIR/results.csv"

echo "prompt,label,round1_score,final_score,rounds,files,loc,time_s,cto_r1,verifier_r1" > "$CSV_FILE"

echo "=========================================================="
echo "  STRESS TEST: 10 PROMPTS — $(date)"
echo "  Config: all-32b coder, 30b security+critique, devstral CTO"
echo "  Results: $RESULTS_DIR"
echo "=========================================================="

for i in "${!PROMPTS[@]}"; do
    PROMPT="${PROMPTS[$i]}"
    LABEL="${LABELS[$i]}"

    echo ""
    echo "=== [$((i+1))/10] $LABEL ==="
    echo "  Prompt: ${PROMPT:0:80}..."

    # Offload all models
    for m in "devstral-small-2:24b-instruct-2512-q8_0" "qwen3-coder:30b-a3b-q8_0" "qwen3-coder-next:q8_0" "qwen2.5-coder:32b"; do
        curl -s http://localhost:11434/api/generate -d "{\"model\":\"$m\",\"keep_alive\":0}" > /dev/null 2>&1
    done
    sleep 2

    START=$(date +%s)

    # Run mission
    OUTPUT=$($BINARY mission "$PROMPT" --preset premium --auto 2>&1)

    END=$(date +%s)
    ELAPSED=$((END - START))

    # Save full output
    echo "$OUTPUT" > "$RESULTS_DIR/${LABEL}_output.txt"

    # Extract metrics from output
    FINAL_SCORE=$(echo "$OUTPUT" | grep -oP 'RESULT: \w+ \(\K[0-9.]+' | tail -1 || echo "0")
    if [ -z "$FINAL_SCORE" ]; then
        FINAL_SCORE=$(echo "$OUTPUT" | grep -oE '[0-9]+\.[0-9]+/10' | tail -1 | cut -d/ -f1 || echo "0")
    fi

    R1_SCORE=$(echo "$OUTPUT" | grep "ROUND 1" | grep -oE '[0-9]+\.[0-9]+' | head -1 || echo "0")
    ROUNDS=$(echo "$OUTPUT" | grep -c "ROUND " || echo "0")
    FILES=$(echo "$OUTPUT" | grep "Files:" | grep -oE 'Files: [0-9]+' | head -1 | grep -oE '[0-9]+' || echo "0")
    LOC=$(echo "$OUTPUT" | grep "LOC:" | grep -oE 'LOC: [0-9]+' | head -1 | grep -oE '[0-9]+' || echo "0")
    CTO_R1=$(echo "$OUTPUT" | grep -A1 "ROUND 1" | grep "CTO:" | grep -oE 'APPROVE|REJECT' | head -1 || echo "N/A")
    VERIFIER_R1=$(echo "$OUTPUT" | grep -A1 "ROUND 1" | grep "Verifier:" | grep -oE '[0-9]+\.[0-9]+' | head -1 || echo "0")

    echo "  Score: $FINAL_SCORE | Rounds: $ROUNDS | Files: $FILES | LOC: $LOC | Time: ${ELAPSED}s | CTO: $CTO_R1"

    # Save to CSV
    echo "$PROMPT,$LABEL,$R1_SCORE,$FINAL_SCORE,$ROUNDS,$FILES,$LOC,$ELAPSED,$CTO_R1,$VERIFIER_R1" >> "$CSV_FILE"

    # Quick summary
    echo "  [$LABEL] score=$FINAL_SCORE rounds=$ROUNDS files=$FILES time=${ELAPSED}s cto=$CTO_R1" >> "$SUMMARY_FILE"
done

echo ""
echo "=========================================================="
echo "  RESULTS SUMMARY"
echo "=========================================================="
cat "$SUMMARY_FILE"
echo ""
echo "  CSV: $CSV_FILE"
echo "  Full outputs: $RESULTS_DIR/"
echo "=========================================================="
