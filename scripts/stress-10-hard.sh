#!/bin/bash
# Stress test: 10 hard prompts (C6-C9) to test the pipeline on complex tasks
# Usage: bash scripts/stress-10-hard.sh
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$SCRIPT_DIR"
BINARY="$SCRIPT_DIR/target/release/battlecommand-forge"
RESULTS_DIR="$SCRIPT_DIR/output/stress-hard-$(date +%Y%m%d_%H%M%S)"
mkdir -p "$RESULTS_DIR"

PROMPTS=(
    "Build a Python FastAPI REST API for a blog platform with posts, comments, tags, and user authentication. Use SQLite with SQLAlchemy async. Include JWT auth, pagination, and pytest tests."
    "Build a Python library that implements a task queue with Redis backend. Support delayed tasks, retries with exponential backoff, dead letter queue, and task priorities. Include comprehensive tests."
    "Build a Python FastAPI microservice for invoice management. CRUD for invoices with line items, PDF generation, email sending via SMTP, and status tracking (draft/sent/paid/overdue). SQLite backend. Include tests."
    "Build a Python CLI tool for database migrations. Support creating migration files, applying/reverting migrations, migration history, and dry-run mode. Work with SQLite. Include tests."
    "Build a Python FastAPI REST API for a file storage service. Upload files with metadata, download by ID, list with filtering, delete with soft-delete, and storage quota tracking. Use SQLite for metadata. Include tests."
    "Build a Python library for a rule engine that evaluates business rules defined in JSON. Support conditions (AND/OR/NOT), comparisons, nested rules, and custom functions. Include comprehensive tests."
    "Build a Python FastAPI webhook delivery service. Register webhook URLs, queue events, deliver with retries and exponential backoff, track delivery status, and provide delivery logs. SQLite backend. Include tests."
    "Build a Python FastAPI REST API for a project management tool. Projects, tasks with assignments, milestones, time tracking entries, and status workflows (todo/in-progress/review/done). SQLite backend. Include tests."
    "Build a Python library for parsing and transforming log files. Support multiple formats (JSON, Apache, syslog), filtering by date/level/pattern, aggregation, and output to JSON/CSV. Include tests."
    "Build a Python FastAPI notification service with multiple channels. Support email (SMTP), webhook, and in-app notifications. Template rendering, user preferences, delivery tracking. SQLite backend. Include tests."
)

LABELS=(
    "C7_blog_api"
    "C7_task_queue"
    "C8_invoice_api"
    "C6_db_migrations"
    "C7_file_storage"
    "C6_rule_engine"
    "C7_webhook_service"
    "C8_project_mgmt"
    "C6_log_parser"
    "C8_notification_svc"
)

SUMMARY_FILE="$RESULTS_DIR/summary.txt"
CSV_FILE="$RESULTS_DIR/results.csv"

echo "prompt,label,final_score,files,loc,time_s" > "$CSV_FILE"

echo "=========================================================="
echo "  STRESS TEST (HARD): 10 C6-C9 PROMPTS — $(date)"
echo "  Config: 32b arch/test, 80B coder, 30b security/critique"
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

    # Extract metrics
    FINAL_SCORE=$(echo "$OUTPUT" | grep -oE '[0-9]+\.[0-9]+/10' | tail -1 | cut -d/ -f1)
    if [ -z "$FINAL_SCORE" ]; then FINAL_SCORE="0"; fi
    FILES=$(echo "$OUTPUT" | grep "Files:" | grep -oE 'Files: [0-9]+' | head -1 | grep -oE '[0-9]+')
    if [ -z "$FILES" ]; then FILES="0"; fi
    LOC=$(echo "$OUTPUT" | grep "LOC:" | grep -oE 'LOC: [0-9]+' | head -1 | grep -oE '[0-9]+')
    if [ -z "$LOC" ]; then LOC="0"; fi

    # Round scores
    SCORES=$(grep -oE 'critique [0-9.]+ \* 0\.7 \+ verifier [0-9.]+ \* 0\.3 = [0-9.]+' <<< "$OUTPUT" | awk '!seen[$0]++' | grep -oE '= [0-9.]+' | grep -oE '[0-9.]+' | tr '\n' '>' | sed 's/>$//')

    echo "  Score: $FINAL_SCORE | Files: $FILES | LOC: $LOC | Time: ${ELAPSED}s | Rounds: [$SCORES]"

    echo "$PROMPT,$LABEL,$FINAL_SCORE,$FILES,$LOC,$ELAPSED" >> "$CSV_FILE"
    echo "  [$LABEL] score=$FINAL_SCORE files=$FILES loc=$LOC time=${ELAPSED}s rounds=[$SCORES]" >> "$SUMMARY_FILE"
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
