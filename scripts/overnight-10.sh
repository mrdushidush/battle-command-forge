#!/bin/bash
# Overnight 10-mission stress test — same prompts as March 22 baseline
# Tests: deps-in-project.dependencies, asyncio_mode=auto, requirements.txt fallback
# Run: bash scripts/overnight-10.sh

set -e
BIN="./target/release/battlecommand-forge"
PRESET="premium"
LOG="output/overnight-$(date +%Y%m%d-%H%M).log"

echo "=== Overnight 10-Mission Test ($(date)) ===" | tee "$LOG"
echo "Testing: coder prompt deps fix + verifier optional-deps + hypothesis" | tee -a "$LOG"
echo "" | tee -a "$LOG"

run_mission() {
    local num=$1
    local prompt=$2
    echo "--- Mission $num/10: $prompt ---" | tee -a "$LOG"
    $BIN mission "$prompt" --preset $PRESET --auto --voice 2>&1 | tee -a "$LOG"
    echo "" | tee -a "$LOG"
}

# Same 10 missions as March 22 baseline (CLAUDE.md)
run_mission 1  "Build a simple Python FastAPI CRUD API for managing books with title, author, and ISBN"
run_mission 2  "Build a Python CLI tool that converts CSV files to JSON with column filtering and type inference"
run_mission 3  "Build a Python FastAPI authentication service with JWT login, refresh tokens, password hashing, and role-based access control"
run_mission 4  "Build a Python WebSocket chat server with rooms, nicknames, and message history using FastAPI"
run_mission 5  "Build a Python FastAPI todo app with CRUD, categories, due dates, and SQLite persistence"
run_mission 6  "Build a Python file monitoring tool that watches a directory for changes and logs events with timestamps"
run_mission 7  "Build a Python FastAPI URL shortener with click tracking and expiration"
run_mission 8  "Build a Python configuration file parser library supporting JSON, YAML, TOML, and environment variable overrides"
run_mission 9  "Build a Python FastAPI e-commerce API with products, cart, orders, and inventory management"
run_mission 10 "Build a Python FastAPI health check endpoint that monitors database connectivity, disk space, and memory usage"

echo "=== Complete ($(date)) ===" | tee -a "$LOG"
echo "Log: $LOG" | tee -a "$LOG"
