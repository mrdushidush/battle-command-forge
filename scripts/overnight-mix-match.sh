#!/bin/bash
# Overnight experiment: 4 model mix-match configurations on the same C8 prompt
# Usage: bash scripts/overnight-mix-match.sh

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$SCRIPT_DIR"
BINARY="$SCRIPT_DIR/target/release/battlecommand-forge"
PROMPT="Build a Python FastAPI authentication service with JWT login, refresh tokens, password hashing, and role-based access control. Use SQLite with SQLAlchemy async. Include comprehensive tests."

RESULTS_DIR="$SCRIPT_DIR/output/mix-match-$(date +%Y%m%d_%H%M%S)"
mkdir -p "$RESULTS_DIR"

# Save original config
cp .battlecommand/models.toml .battlecommand/models.toml.original

run_experiment() {
    local NAME="$1"
    local CONFIG="$2"

    echo ""
    echo "============================================================"
    echo "  EXPERIMENT: $NAME"
    echo "  $(date)"
    echo "============================================================"

    # Write config
    echo "$CONFIG" > .battlecommand/models.toml

    # Show config
    echo "Config:"
    grep "model = " .battlecommand/models.toml | while read line; do echo "  $line"; done

    # Offload all models
    for m in "devstral-small-2:24b-instruct-2512-q8_0" "qwen3-coder:30b-a3b-q8_0" "qwen3-coder-next:q8_0" "qwen2.5-coder:32b"; do
        curl -s http://localhost:11434/api/generate -d "{\"model\":\"$m\",\"keep_alive\":0}" > /dev/null 2>&1
    done
    sleep 3

    START=$(date +%s)
    OUTPUT=$($BINARY mission "$PROMPT" --preset premium --auto 2>&1)
    END=$(date +%s)
    ELAPSED=$((END - START))

    echo "$OUTPUT" > "$RESULTS_DIR/${NAME}_output.txt"

    # Extract metrics
    SCORE=$(echo "$OUTPUT" | grep -oE 'best: [0-9.]+' | grep -oE '[0-9.]+' | head -1)
    if [ -z "$SCORE" ]; then
        SCORE=$(echo "$OUTPUT" | grep -oE '[0-9]+\.[0-9]+/10' | tail -1 | cut -d/ -f1)
    fi
    FILES=$(echo "$OUTPUT" | grep "Files:" | grep -oE 'Files: [0-9]+' | head -1 | grep -oE '[0-9]+')
    LOC=$(echo "$OUTPUT" | grep "LOC:" | grep -oE 'LOC: [0-9]+' | head -1 | grep -oE '[0-9]+')
    TESTS_PASS=$(echo "$OUTPUT" | grep "pytest:" | grep -oE '[0-9]+ passed' | tail -1 | grep -oE '[0-9]+')
    TESTS_FAIL=$(echo "$OUTPUT" | grep "pytest:" | grep -oE '[0-9]+ failed' | tail -1 | grep -oE '[0-9]+')

    SCORES=$(grep -oE 'critique [0-9.]+ \* [0-9.]+ \+ verifier [0-9.]+ \* [0-9.]+ = [0-9.]+' <<< "$OUTPUT" | awk '!seen[$0]++' | grep -oE '= [0-9.]+' | grep -oE '[0-9.]+' | tr '\n' '>' | sed 's/>$//')

    echo ""
    echo "  RESULT: score=$SCORE | files=$FILES | loc=$LOC | tests=${TESTS_PASS:-0}p/${TESTS_FAIL:-0}f | time=${ELAPSED}s"
    echo "  Rounds: [$SCORES]"
    echo ""

    echo "$NAME,$SCORE,$FILES,$LOC,${TESTS_PASS:-0},${TESTS_FAIL:-0},$ELAPSED,$SCORES" >> "$RESULTS_DIR/results.csv"
}

echo "name,score,files,loc,tests_pass,tests_fail,time_s,rounds" > "$RESULTS_DIR/results.csv"

# ─── EXPERIMENT 1: All Local (baseline) ───
run_experiment "1_all_local" '
preset = "premium"
[architect]
model = "qwen2.5-coder:32b"
context_size = 32768
max_predict = 4096
[tester]
model = "qwen2.5-coder:32b"
context_size = 32768
max_predict = 8192
[coder]
model = "qwen3-coder-next:q8_0"
context_size = 131072
max_predict = 32768
[security]
model = "qwen3-coder:30b-a3b-q8_0"
context_size = 32768
max_predict = 1024
[critique]
model = "qwen3-coder:30b-a3b-q8_0"
context_size = 32768
max_predict = 1024
[cto]
model = "devstral-small-2:24b-instruct-2512-q8_0"
context_size = 32768
max_predict = 1024
[complexity]
model = "qwen3-coder:30b-a3b-q8_0"
max_predict = 1024
'

# ─── EXPERIMENT 2: All Local + Sonnet Coder ───
run_experiment "2_local_sonnet_coder" '
preset = "premium"
[architect]
model = "qwen2.5-coder:32b"
context_size = 32768
max_predict = 4096
[tester]
model = "qwen2.5-coder:32b"
context_size = 32768
max_predict = 8192
[coder]
model = "claude-sonnet-4-20250514"
context_size = 200000
max_predict = 16384
[security]
model = "qwen3-coder:30b-a3b-q8_0"
context_size = 32768
max_predict = 1024
[critique]
model = "qwen3-coder:30b-a3b-q8_0"
context_size = 32768
max_predict = 1024
[cto]
model = "devstral-small-2:24b-instruct-2512-q8_0"
context_size = 32768
max_predict = 1024
[complexity]
model = "qwen3-coder:30b-a3b-q8_0"
max_predict = 1024
'

# ─── EXPERIMENT 3: Opus Architect + Local Everything Else ───
run_experiment "3_opus_architect_local" '
preset = "premium"
[architect]
model = "claude-opus-4-6"
context_size = 200000
max_predict = 4096
[tester]
model = "qwen2.5-coder:32b"
context_size = 32768
max_predict = 8192
[coder]
model = "qwen3-coder-next:q8_0"
context_size = 131072
max_predict = 32768
[security]
model = "qwen3-coder:30b-a3b-q8_0"
context_size = 32768
max_predict = 1024
[critique]
model = "qwen3-coder:30b-a3b-q8_0"
context_size = 32768
max_predict = 1024
[cto]
model = "devstral-small-2:24b-instruct-2512-q8_0"
context_size = 32768
max_predict = 1024
[complexity]
model = "qwen3-coder:30b-a3b-q8_0"
max_predict = 1024
'

# ─── EXPERIMENT 4: All Sonnet ───
run_experiment "4_all_sonnet" '
preset = "premium"
[architect]
model = "claude-sonnet-4-20250514"
context_size = 200000
max_predict = 4096
[tester]
model = "claude-sonnet-4-20250514"
context_size = 200000
max_predict = 8192
[coder]
model = "claude-sonnet-4-20250514"
context_size = 200000
max_predict = 16384
[security]
model = "claude-sonnet-4-20250514"
context_size = 200000
max_predict = 1024
[critique]
model = "claude-sonnet-4-20250514"
context_size = 200000
max_predict = 1024
[cto]
model = "claude-sonnet-4-20250514"
context_size = 200000
max_predict = 1024
[complexity]
model = "claude-sonnet-4-20250514"
max_predict = 1024
'

# ─── EXPERIMENT 5: Opus Coder + Local Reviews (best value?) ───
run_experiment "5_opus_coder_local_review" '
preset = "premium"
[architect]
model = "qwen2.5-coder:32b"
context_size = 32768
max_predict = 4096
[tester]
model = "qwen2.5-coder:32b"
context_size = 32768
max_predict = 8192
[coder]
model = "claude-opus-4-6"
context_size = 200000
max_predict = 16384
[security]
model = "qwen3-coder:30b-a3b-q8_0"
context_size = 32768
max_predict = 1024
[critique]
model = "qwen3-coder:30b-a3b-q8_0"
context_size = 32768
max_predict = 1024
[cto]
model = "devstral-small-2:24b-instruct-2512-q8_0"
context_size = 32768
max_predict = 1024
[complexity]
model = "qwen3-coder:30b-a3b-q8_0"
max_predict = 1024
'

# Restore original config
cp .battlecommand/models.toml.original .battlecommand/models.toml
rm .battlecommand/models.toml.original

echo ""
echo "============================================================"
echo "  OVERNIGHT RESULTS — $(date)"
echo "============================================================"
echo ""
printf "%-30s %6s %6s %6s %8s %8s %7s\n" "Experiment" "Score" "Files" "LOC" "TestsP" "TestsF" "Time"
echo "------------------------------------------------------------------------------------"
while IFS=, read -r name score files loc tp tf time rounds; do
    [ "$name" = "name" ] && continue
    printf "%-30s %6s %6s %6s %8s %8s %6ss\n" "$name" "$score" "$files" "$loc" "$tp" "$tf" "$time"
done < "$RESULTS_DIR/results.csv"
echo ""
echo "Full outputs: $RESULTS_DIR/"
echo "============================================================"
