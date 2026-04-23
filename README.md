# BattleCommand Forge

> **Status: stable port at v0.1.0 (April 2026).** Battle-command-forge is the quality-first code-generation branch of an AI-agent project family I've been building since January 2026. This release is a public port of internal pipeline work — the code is field-tested (86/86 unit tests, plus the 10-mission stress suite documented below) but active feature development has slowed while I focus on sibling projects in the same family. Issues and PRs remain welcome. Related: **[claudette](https://github.com/mrdushidush/claudette)** (local-first personal assistant, on crates.io) · **[ABCC](https://github.com/mrdushidush/agent-battle-command-center)** (the godfather — RTS-style TUI for agent orchestration).

**Quality-first AI coding army.** A pure Rust single binary (~3.7 MB) that generates production-grade code using a 9-stage quality pipeline with TDD enforcement, multi-file extraction, surgical fix-pass retry, and a complexity-scaled quality gate.

Give it a mission. Get back a tested, reviewed, production-ready project.

```
$ battlecommand-forge mission "Build a FastAPI CRUD API for managing books with SQLite" --auto

[ROUTER]    Complexity: C4 (moderate) — CRUD + SQLite + FastAPI
[ARCHITECT] Spec: 6 files, 77 lines, TDD plan with 12 test cases
[TESTER]    24 tests written (pytest-asyncio, Pydantic v2 fixtures)
[CODER]     Generating 6 files (single-shot, 80B model)...
[VERIFIER]  venv created, deps installed, ruff clean, 22/24 tests pass
[SECURITY]  OWASP review: no critical issues, score 8.5/10
[CRITIQUE]  DEV=9.0 ARCH=8.5 TEST=8.0 SEC=8.5 DOCS=7.5 → avg 8.3
[CTO]       Approved — coherent, well-structured, ships as-is
[GATE]      Score: 9.1/10 (threshold: 9.2) — FIX ROUND 1
[FIX]       Surgical fix: 1 file (models.py — dual Base bug)
[VERIFIER]  24/24 tests pass
[GATE]      Score: 9.4/10 — SHIPPED ✅

Output: output/fastapi_books_crud/
```

---

## Table of Contents

- [30-Second Quick Start](#30-second-quick-start)
- [Installation](#installation)
- [Your First Mission](#your-first-mission)
- [CLI Commands](#cli-commands)
- [Interactive TUI](#interactive-tui)
- [Presets](#presets)
- [Configuration](#configuration)
- [Environment Variables](#environment-variables)
- [Common Workflows](#common-workflows)
- [How the Pipeline Works](#how-the-pipeline-works)
- [Troubleshooting](#troubleshooting)
- [Architecture](#architecture)

---

## 30-Second Quick Start

```bash
# 1. Build
cargo build --release

# 2. Start Ollama (if not running)
ollama serve &

# 3. Pull a model (7B for fast start — upgrade later)
ollama pull qwen2.5-coder:7b

# 4. Run your first mission
./target/release/battlecommand-forge mission "Create a Python CLI that converts CSV to JSON" --preset fast --auto
```

That's it. Your generated project is in `output/`.

---

## Installation

### Prerequisites

| Requirement | Version | Why |
|-------------|---------|-----|
| **Rust** | 1.75+ | Building the binary |
| **Ollama** | Latest | Running local models |
| **Python** | 3.10+ | Generated code + verifier (creates venvs) |

### Build from Source

```bash
git clone <repo-url>
cd battlecommand-forge
cargo build --release
```

The binary is at `./target/release/battlecommand-forge` (~3.7 MB with LTO + strip).

**Optional:** Copy it to your PATH:

```bash
cp target/release/battlecommand-forge /usr/local/bin/bcf
```

Now you can use `bcf` from anywhere.

### Pull Models

The preset you choose determines which models you need:

```bash
# Fast preset ($0, needs 8GB RAM)
ollama pull qwen2.5-coder:7b

# Balanced preset ($0, needs 20GB RAM)
ollama pull qwen2.5-coder:32b

# Premium preset (best quality, needs 48GB+ VRAM for local models)
ollama pull qwen2.5-coder:32b          # architect
ollama pull qwen3-coder-next:q8_0      # coder (80B)
ollama pull qwen3-coder:30b-a3b-q8_0   # critic
```

Premium also uses cloud APIs — set these if you want the best results:

```bash
# Add to your ~/.zshrc or ~/.bashrc
export ANTHROPIC_API_KEY=sk-ant-...    # Claude Opus/Sonnet (~$0.20-0.30/mission)
export XAI_API_KEY=xai-...             # Grok architect (optional, ~$0.10/mission)
```

### Verify Installation

```bash
battlecommand-forge status
```

```
BattleCommand Forge v1.0.0
Modules: 30 | Pipeline: 9-stage | Gate: 8.0-9.2/10 (scaled) | Fix rounds: 5

Ollama: connected (12 models)
Claude: configured
GitHub: gh authenticated
Workspaces: 47
Total missions: 47 | Avg score: 7.8/10
Total cost: $4.2100
```

---

## Your First Mission

### Example 1: Simple CLI Tool (C3, ~$0)

```bash
battlecommand-forge mission "Build a Python CLI that converts CSV files to JSON, supporting custom delimiters and output formatting" --preset fast --auto
```

**What happens:**
1. Router scores this as C3 (low complexity)
2. Architect writes a concise spec with 4 files
3. Tester writes 8 test cases
4. Coder generates all files in one shot
5. Verifier creates a venv, runs ruff + pytest
6. If tests fail, surgical fix targets the exact broken file
7. Output lands in `output/csv_to_json_cli/`

**Your output directory:**
```
output/csv_to_json_cli/
├── main.py              # CLI entry point (click/argparse)
├── converter.py         # Core conversion logic
├── tests/
│   └── test_converter.py  # 8 test cases
├── requirements.txt
└── pyproject.toml
```

### Example 2: REST API (C5, ~$0.30)

```bash
battlecommand-forge mission "Build a FastAPI REST API for a todo app with SQLite, full CRUD, due dates, priority levels, and filtering" --preset premium --auto
```

### Example 3: Complex Auth Service (C8, ~$0.50)

```bash
battlecommand-forge mission "Build a FastAPI auth service with JWT access+refresh tokens, RBAC (admin/user/viewer), bcrypt passwords, SQLAlchemy async, rate limiting, and CSRF protection" --preset premium --auto
```

### Example 4: Edit Existing Code

```bash
# Point at an existing project and describe what to change
battlecommand-forge edit --path ./my-project "Add pagination to all list endpoints, with page/per_page query params and Link headers"
```

### Example 5: Custom Output Directory

```bash
battlecommand-forge mission "Build a URL shortener" --auto -o ~/projects/url-shortener
```

### Example 6: Use a GitHub Repo as Context

```bash
battlecommand-forge mission "Add WebSocket support for real-time notifications" --repo https://github.com/user/my-api --auto
```

### Example 7: Voice Announcements (macOS)

```bash
battlecommand-forge mission "Build a health check API" --voice --auto
# Announces: "Mission started..." "Quality gate passed..." "Mission complete."
```

---

## CLI Commands

### `mission` — Run the full pipeline

```bash
battlecommand-forge mission "<prompt>" [options]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--preset` | `premium` | Model preset: `fast`, `balanced`, `premium` |
| `--auto` | off | Skip human approval, auto-continue fix rounds |
| `-o, --output` | auto | Custom output directory |
| `--repo` | — | Clone a GitHub repo as context |
| `--path` | — | Use a local directory as context |
| `--voice` | off | macOS TTS announcements |
| `--architect-model` | — | Override architect model |
| `--tester-model` | — | Override tester model |
| `--coder-model` | — | Override coder model |
| `--reviewer-model` | — | Override security + critique + CTO |

**Examples:**

```bash
# Fully automatic, premium quality
battlecommand-forge mission "Build a markdown blog engine" --preset premium --auto

# Fast iteration, local only, no API costs
battlecommand-forge mission "Build a todo CLI" --preset fast --auto

# Manual mode — approve each stage
battlecommand-forge mission "Build an auth service"

# Override just the coder model
battlecommand-forge mission "Build a chat server" --auto --coder-model claude-sonnet-4-6
```

### `chat` — CTO Research Chat (CLI)

```bash
battlecommand-forge chat
```

Interactive REPL where you chat with the CTO agent to plan missions before launching. Supports tool calling (web search, file reading, directory listing), conversation history, and launching missions directly from chat.

```
$ battlecommand-forge chat --preset premium

BattleCommand Forge — CTO Chat (claude-sonnet-4-6)
Plan your mission, research architecture, or ask anything.
Type /mission <prompt> to launch. /clear to reset. /quit to exit.

> What's the best database for a real-time chat app?
  [web_search: "best database real-time chat 2026"]
  [web_search → Redis for pub/sub + message queue, PostgreSQL for persistence...]

For a real-time chat app, I recommend a dual-database approach:
- **Redis** for pub/sub messaging, presence tracking, and room state
- **PostgreSQL** (async via SQLAlchemy) for message history and user accounts
...

> /mission Build a WebSocket chat server with Redis pub/sub, PostgreSQL message history, FastAPI, rooms, and user presence

Launching mission: Build a WebSocket chat server...
[ROUTER] Complexity: C7 (high)
...
```

**Chat commands:**

| Command | Action |
|---------|--------|
| `/mission <prompt>` | Launch a mission from chat |
| `/clear` | Clear conversation history |
| `/compress` | Compact long history |
| `/help` | List commands |
| `/quit` | Exit chat |

### `tui` — Interactive Terminal UI

```bash
battlecommand-forge tui
```

Full-featured 6-tab interface with CTO research chat, model picker, hardware monitoring, and 14 slash commands. See [Interactive TUI](#interactive-tui) for details.

### `edit` — Modify Existing Code

```bash
battlecommand-forge edit --path <directory> "<what to change>"
```

```bash
# Add tests to an existing project
battlecommand-forge edit --path ./my-api "Add comprehensive pytest tests for all endpoints"

# Refactor a module
battlecommand-forge edit --path ./my-lib "Refactor the parser module to use the visitor pattern"
```

### `verify` — Run Quality Checks

```bash
battlecommand-forge verify --path <directory> [--lang python]
```

Creates a venv, installs deps, runs ruff (linting) and pytest. Shows score, test results, lint issues, and secret detection.

```bash
# Verify a generated project
battlecommand-forge verify --path output/todo_crud

# Verify your own project
battlecommand-forge verify --path ~/projects/my-api --lang python
```

### `models` — List, Benchmark, and Compare

```bash
# List all Ollama models
battlecommand-forge models list

# Benchmark a specific model (speed + quality test)
battlecommand-forge models benchmark qwen2.5-coder:32b

# Show preset configurations
battlecommand-forge models presets
```

### `stress` — Run the Stress Test Suite

```bash
# Run all 21 graded tasks (C4-C9)
battlecommand-forge stress --tasks 21 --preset premium

# Quick smoke test (5 tasks)
battlecommand-forge stress --tasks 5 --preset fast
```

### `status` — System Health Check

```bash
battlecommand-forge status
```

Shows Ollama connection, API key status, workspace count, mission history, and total cost.

### `report` — View Pipeline Reports

```bash
# List all reports
battlecommand-forge report list

# Show the latest report (detailed breakdown)
battlecommand-forge report show
```

### `audit` — View Audit Log

```bash
# Last 20 entries
battlecommand-forge audit --limit 20
```

### `settings` — Model Configuration

```bash
# Show resolved config for a preset
battlecommand-forge settings show --preset premium

# Generate default .battlecommand/models.toml
battlecommand-forge settings init
```

### `github` — Push and Create PRs

```bash
# Check if gh CLI is available
battlecommand-forge github check

# Push a workspace
battlecommand-forge github push --workspace output/my_project --branch main

# Create a PR
battlecommand-forge github create-pr --workspace output/my_project --title "Add books API" --base main
```

### `hw` — Hardware Metrics

```bash
battlecommand-forge hw
```

Shows CPU, RAM, Ollama status, and VRAM usage.

---

## Interactive TUI

Launch with:

```bash
battlecommand-forge tui
```

### 6 Tabs

| Tab | Key | What's There |
|-----|:---:|-------------|
| **Chat** | `1` | CTO research agent with 10 tools |
| **Queue** | `2` | Mission queue and status |
| **Models** | `3` | Available Ollama models |
| **Code** | `4` | Generated code viewer |
| **HW** | `5` | Live hardware metrics |
| **Log** | `6` | Pipeline activity log |

### Chat with the CTO Agent

The Chat tab connects you to an AI CTO agent with tool access. It can search the web, read files, list directories, and launch missions — all from the chat interface.

```
> What's the best way to structure a FastAPI app with SQLAlchemy async?

CTO: For a production FastAPI + async SQLAlchemy setup, I recommend...
[web_search: "FastAPI async SQLAlchemy best practices 2026"]
...here's the recommended structure:
  app/
    main.py         # FastAPI app + middleware
    database.py     # Single AsyncEngine + AsyncSession factory
    models.py       # SQLAlchemy ORM models (import Base from database)
    schemas.py      # Pydantic v2 models (UserRead, UserCreate)
    routes/         # One file per resource
    dependencies.py # get_db session dependency
...

> /mission Build that FastAPI app with user CRUD and SQLAlchemy async

[Mission launched: Build that FastAPI app...]
```

**CTO Tools (10):**

| Tool | What It Does |
|------|-------------|
| `web_search` | Search the web (Brave Search or DuckDuckGo fallback) |
| `web_fetch` | Fetch and read a web page |
| `read_file` | Read any local file |
| `list_files` | List directory contents |
| `run_mission` | Launch a mission from chat |
| `refine_prompt` | Improve a mission prompt before running |
| `verify_project` | Run verifier on a project directory |
| `list_reports` | List pipeline reports |
| `open_browser` | Open a URL in the default browser |
| `status` | Show system status |

### Slash Commands

Type these in the Chat input:

| Command | What It Does |
|---------|-------------|
| `/mission <prompt>` | Launch a mission |
| `/verify [path]` | Run verifier (default: latest output) |
| `/report [list\|show]` | View pipeline reports |
| `/audit [n]` | Show audit log (default: 10) |
| `/preset <name>` | Switch preset (fast/balanced/premium) |
| `/cost` | Show total API cost |
| `/settings` | Open model picker overlay |
| `/clear` | Clear chat + CTO history |
| `/compress` | Compact CTO conversation history |
| `/models` | Switch to Models tab |
| `/hw` | Switch to Hardware tab |
| `/status` | Show workspace/system info |
| `/snake` | Play snake! |
| `/space` | Play Space Invaders! |
| `/help` | List all commands |

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `1`-`6` | Switch tabs (when input is empty) |
| `Tab` | Cycle through tabs |
| `PgUp` / `PgDn` | Scroll chat ±20 lines |
| `Up` / `Down` | Scroll chat ±3 lines (when input is empty) |
| `Home` / `End` | Scroll to top/bottom or cursor start/end |
| `Esc` | Clear input or quit |
| `Ctrl+C` | Quit |

---

## Presets

Presets control which models are used for each pipeline role.

### Fast — $0, Runs Anywhere

```bash
battlecommand-forge mission "..." --preset fast
```

All roles use `qwen2.5-coder:7b`. Needs ~8GB RAM. Best for quick iteration and testing.

### Balanced — $0, Better Quality

```bash
battlecommand-forge mission "..." --preset balanced
```

Uses `qwen2.5-coder:32b` for architect/coder. Needs ~20GB RAM. Good balance of speed and quality.

### Premium — ~$0.30/mission, Best Quality

```bash
battlecommand-forge mission "..." --preset premium
```

Dream Team v3: local 32B architect + Opus tester + local 80B coder + Sonnet fix/CTO + local 30B critics. Needs 48GB+ VRAM and `ANTHROPIC_API_KEY`.

| Role | Model | Provider | Cost |
|------|-------|----------|:----:|
| Architect | qwen2.5-coder:32b | Local | $0 |
| Tester | claude-opus-4-6 | Anthropic | ~$0.20 |
| Coder | qwen3-coder-next:q8_0 (80B) | Local | $0 |
| Fix Coder | claude-sonnet-4-6 | Anthropic | ~$0.05 |
| Security | qwen3-coder:30b-a3b-q8_0 | Local | $0 |
| Critique | qwen3-coder:30b-a3b-q8_0 | Local | $0 |
| CTO | claude-sonnet-4-6 | Anthropic | ~$0.05 |

**Upgrade to full Dream Team v3** — add Grok reasoning architect for complex missions (C7+):

```bash
export XAI_API_KEY=xai-...
battlecommand-forge mission "..." --preset premium --architect-model grok-4.20-reasoning --auto
```

### Comparing Presets

| | Fast | Balanced | Premium |
|---|:---:|:---:|:---:|
| Avg score | 6.5/10 | 7.2/10 | 8.5/10 |
| Tests passing | ~40% | ~60% | ~85% |
| Cost per mission | $0 | $0 | ~$0.30 |
| Min RAM/VRAM | 8 GB | 20 GB | 48 GB |
| Time per mission | 3-8 min | 8-15 min | 8-12 min |

---

## Configuration

### Priority Order

Configuration resolves in this order (last wins):

```
Preset defaults → Environment variables → .battlecommand/models.toml → CLI flags
```

### Config File

Generate the default config:

```bash
battlecommand-forge settings init
```

This creates `.battlecommand/models.toml`:

```toml
# BattleCommand Forge — Model Configuration
preset = "premium"

# Uncomment to customize any role:

# [architect]
# model = "qwen2.5-coder:32b"
# context_size = 32768
# max_predict = 4096

# [tester]
# model = "claude-opus-4-6"
# context_size = 200000
# max_predict = 8192

# [coder]
# model = "qwen3-coder-next:q8_0"
# context_size = 65536
# max_predict = 32768

# [fix_coder]
# model = "claude-sonnet-4-6"

# [security]
# model = "qwen3-coder:30b-a3b-q8_0"

# [critique]
# model = "qwen3-coder:30b-a3b-q8_0"

# [cto]
# model = "claude-sonnet-4-6"
```

### Per-Role Options

Each role section supports:

| Field | Default | Description |
|-------|---------|-------------|
| `model` | from preset | Model name (e.g. `claude-sonnet-4-6`, `qwen2.5-coder:32b`) |
| `context_size` | 32768 | Context window in tokens |
| `max_predict` | 8192 | Max output tokens |

Provider is auto-detected: `claude-*` and `grok-*` models → cloud, everything else → local (Ollama).

### CLI Overrides (Highest Priority)

```bash
# Override a single role
battlecommand-forge mission "..." --coder-model claude-sonnet-4-6

# Override all reviewers at once
battlecommand-forge mission "..." --reviewer-model claude-sonnet-4-6

# Mix and match
battlecommand-forge mission "..." \
  --architect-model grok-4.20-reasoning \
  --tester-model claude-opus-4-6 \
  --coder-model qwen3-coder-next:q8_0 \
  --reviewer-model claude-sonnet-4-6
```

---

## Environment Variables

### Required (for cloud models)

```bash
export ANTHROPIC_API_KEY=sk-ant-...    # Claude Opus/Sonnet
export XAI_API_KEY=xai-...             # Grok models
```

### Optional

| Variable | Example | Description |
|----------|---------|-------------|
| `OLLAMA_HOST` | `192.168.1.100:11434` | Remote Ollama URL |
| `BRAVE_API_KEY` | `BSA...` | CTO web search (falls back to DuckDuckGo) |
| `ARCHITECT_MODEL` | `grok-4.20-reasoning` | Override architect |
| `TESTER_MODEL` | `claude-opus-4-6` | Override tester |
| `CODER_MODEL` | `qwen3-coder-next:q8_0` | Override coder |
| `FIX_CODER_MODEL` | `claude-sonnet-4-6` | Override fix coder |
| `SECURITY_MODEL` | `qwen3-coder:30b-a3b-q8_0` | Override security reviewer |
| `CRITIQUE_MODEL` | `qwen3-coder:30b-a3b-q8_0` | Override critique panel |
| `CTO_MODEL` | `claude-sonnet-4-6` | Override CTO |
| `REVIEWER_MODEL` | `claude-sonnet-4-6` | Override security + critique + CTO together |

You can also use a `.env` file in the project root — it's loaded automatically.

### Remote Ollama (Cloud GPU)

Run models on a remote machine with a beefy GPU:

```bash
# On the remote machine (H200, A100, etc.):
bash scripts/h200-setup.sh

# From your Mac:
OLLAMA_HOST=192.168.1.100:11434 battlecommand-forge mission "..." --preset premium --auto
```

---

## Common Workflows

### Workflow 1: Quick Prototype → Polish

```bash
# 1. Fast draft to see the shape
battlecommand-forge mission "Build a URL shortener with FastAPI" --preset fast --auto

# 2. Check what needs fixing
battlecommand-forge verify --path output/url_shortener

# 3. Re-run with premium for production quality
battlecommand-forge mission "Build a URL shortener with FastAPI" --preset premium --auto
```

### Workflow 2: Research → Build

```bash
# 1. Launch TUI
battlecommand-forge tui

# 2. Chat with CTO to research architecture
> What are the best practices for WebSocket chat servers in Python?
> What database should I use for real-time message storage?

# 3. Launch mission from chat when ready
> /mission Build a WebSocket chat server with rooms, user presence, message history in Redis, and FastAPI HTTP endpoints for room management
```

### Workflow 3: Iterate on Existing Code

```bash
# 1. Generate initial project
battlecommand-forge mission "Build a config parser library" --auto -o ~/projects/config-lib

# 2. Add features with edit mode
battlecommand-forge edit --path ~/projects/config-lib "Add YAML support alongside TOML and JSON"
battlecommand-forge edit --path ~/projects/config-lib "Add schema validation with descriptive error messages"

# 3. Verify after each edit
battlecommand-forge verify --path ~/projects/config-lib
```

### Workflow 4: Stress Test Your Pipeline

```bash
# Run graded tasks to benchmark your model setup
battlecommand-forge stress --tasks 10 --preset premium

# Check reports
battlecommand-forge report list
```

### Workflow 5: Ship to GitHub

```bash
# 1. Generate project
battlecommand-forge mission "Build a markdown blog engine" --auto

# 2. Push to GitHub
battlecommand-forge github push --workspace output/markdown_blog --branch main

# 3. Create a PR
battlecommand-forge github create-pr \
  --workspace output/markdown_blog \
  --title "feat: markdown blog engine" \
  --base main
```

---

## How the Pipeline Works

```
 ┌─────────┐   ┌───────────┐   ┌────────┐   ┌───────┐   ┌──────────┐
 │ ROUTER  │──▶│ ARCHITECT │──▶│ TESTER │──▶│ CODER │──▶│ VERIFIER │
 │  C1-C10 │   │  ADR+spec │   │  TDD   │   │ code  │   │ venv+test│
 └─────────┘   └───────────┘   └────────┘   └───────┘   └──────────┘
                                                               │
                  ┌──────────────────────────────────────────────┘
                  ▼
           ┌──────────┐   ┌──────────┐   ┌─────┐   ┌──────────────┐
           │ SECURITY │──▶│ CRITIQUE │──▶│ CTO │──▶│ QUALITY GATE │
           │  OWASP   │   │ 5 scores │   │ ok? │   │  ship/fix?   │
           └──────────┘   └──────────┘   └─────┘   └──────────────┘
                                                          │
                                                    ┌─────┴─────┐
                                                    │           │
                                                  PASS        FAIL
                                                    │           │
                                                  SHIP    ┌─────▼─────┐
                                                    ✅     │ SURGICAL  │
                                                          │ FIX LOOP  │──▶ back to CODER
                                                          │ (≤5 rounds)│
                                                          └───────────┘
```

### Stage Details

| # | Stage | What It Does | Model |
|---|-------|-------------|-------|
| 1 | **Router** | Scores complexity C1-C10 (rules + AI) | Local small |
| 2 | **Architect** | Writes ADR, file manifest, TDD test plan | Local 32B |
| 3 | **Tester** | Writes complete test suite BEFORE code | Opus ($0.20) |
| 4 | **Coder** | Generates all files in single shot | Local 80B |
| 5 | **Verifier** | Creates venv, pip install, ruff, pytest | — |
| 6 | **Security** | OWASP Top 10 review | Local/Sonnet |
| 7 | **Critique** | 5-in-1 scoring: DEV/ARCH/TEST/SEC/DOCS | Local/Sonnet |
| 8 | **CTO** | Mission-level coherence check | Sonnet ($0.05) |
| 9 | **Gate** | Ships if `critique*0.4 + verifier*0.6 ≥ threshold` | — |

### Surgical Fix Loop

When the quality gate fails, the pipeline doesn't regenerate everything. It:

1. **Traces imports** — finds exactly which files are broken (NameError, ImportError, etc.)
2. **Fixes surgically** — each broken file gets its own LLM call with specific error context
3. **Preserves clean code** — files that pass stay untouched
4. **Tracks progress** — if score declines 2+ rounds in a row, restores the best round's files
5. **Limits scope** — fix rounds fix bugs only, never add features (prevents regression)

### Quality Gate Thresholds

| Complexity | Threshold | Why |
|:----------:|:---------:|-----|
| C1-C6 | 9.2/10 | Simple tasks should be near-perfect |
| C7-C8 | 8.5/10 | Complex but achievable with fix rounds |
| C9-C10 | 8.0/10 | Very complex — reward functional code |

---

## Troubleshooting

### "Ollama: not running"

```bash
# Start Ollama
ollama serve

# Or check if it's running
curl http://localhost:11434/api/tags
```

### "No models available"

```bash
# Pull the model you need
ollama pull qwen2.5-coder:7b

# List what's installed
ollama list
```

### Tests always fail / score stuck below 7

This usually means the tester model is too weak. The #1 improvement is using Opus as the tester:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
battlecommand-forge mission "..." --preset premium --auto
```

Opus writes correct test fixtures on the first try (~$0.20). Local 32B testers write tests that never run.

### "Connection refused" with remote Ollama

```bash
# Make sure Ollama is listening on all interfaces (not just localhost)
OLLAMA_HOST=0.0.0.0:11434 ollama serve

# Test from your Mac
curl http://<remote-ip>:11434/api/tags
```

### Build fails with old Rust

```bash
rustup update
# Needs Rust 1.75+
```

### High API costs

Switch to a cheaper preset or use all-local models:

```bash
# $0 — all local
battlecommand-forge mission "..." --preset fast --auto

# Or just make the coder local, keep cloud reviews
battlecommand-forge mission "..." --preset premium --coder-model qwen2.5-coder:32b --auto
```

Check your spend anytime:

```bash
battlecommand-forge status   # shows total cost
# Or in TUI: /cost
```

### Generated code has import errors

This is the most common issue. The pipeline handles it automatically via surgical fix rounds, but if you see it in the final output:

```bash
# Re-verify to see exact errors
battlecommand-forge verify --path output/my_project

# The error patterns are saved for future runs
cat .battlecommand/failure_patterns.md
```

---

## Architecture

### 30 Modules, 14,000+ Lines of Rust

| Module | Purpose |
|--------|---------|
| `mission.rs` | 9-stage pipeline orchestration + surgical fix loop |
| `tui.rs` | 6-tab interactive TUI with CTO chat + 15 slash commands |
| `llm.rs` | Claude API + Ollama + Grok client + streaming + tool calling |
| `cto.rs` | CTO agent with 10 tools (web search, file read, verify, etc.) |
| `verifier.rs` | Venv creation + pip install + ruff + pytest |
| `codegen.rs` | Multi-file extraction from LLM output |
| `model_config.rs` | Per-role model config (preset → env → TOML → CLI) |
| `model_picker.rs` | Interactive model selection UI overlay |
| `router.rs` | Dual complexity scoring (rules + AI) |
| `editor.rs` | Edit existing codebases via LLM |
| `sandbox.rs` | Sandboxed execution, timeouts, env stripping |
| `memory.rs` | Learnings + few-shot examples + context injection |
| `enterprise.rs` | Audit logging, cost tracking, RBAC |
| `report.rs` | Pipeline report generation + viewer |
| `hardware.rs` | CPU/RAM/VRAM/Ollama monitoring |
| `models.rs` | Model listing, benchmarking, VRAM estimation |
| `workspace.rs` | Isolated git workspaces per mission |
| `swebench.rs` | SWE-bench evaluation: ReAct agent loop, dataset handling |
| `swebench_tools.rs` | 7 ReAct tools: read_file, grep, list_dir, run_command, write, edit, submit |
| `swebench_eval.rs` | SWE-bench report generation with per-repo breakdown |
| `benchmark.rs` | Multi-model benchmark framework (5 graded missions) |
| `swarm.rs` | Swarm mode: planner→coder→QA iteration with best-version selection |
| `custom_commands.rs` | User-defined commands from `.battlecommand/commands/*.md` |
| `stress.rs` | 21-task stress test suite (C4-C9) |
| `snake.rs` | Easter egg snake game |
| `space.rs` | Easter egg Space Invaders game |
| `db.rs` | Mission history (JSON file-based) |
| `context.rs` | Context compaction at 95% capacity |
| `github.rs` | GitHub push/PR via gh CLI |
| `voice.rs` | macOS TTS announcements |

### Key Design Decisions

- **Pure Rust, no Python bridge** — single binary, no runtime deps
- **TDD-first pipeline** — tests are written BEFORE code, not after
- **Surgical fixes over regeneration** — fix rounds target only broken files, preserving working code
- **Mix-and-match models** — each pipeline role can use a different model (local or cloud)
- **Venv per project** — generated code runs in isolated environments
- **Streaming everything** — architect/tester/coder output streams live to terminal
- **Quality gate is intentionally high** — pushes models to produce production-grade output

---

## License

Apache-2.0. See [LICENSE](LICENSE).
