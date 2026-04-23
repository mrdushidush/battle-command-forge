# BattleCommand Forge — Developer Guide

## What This Is

Quality-first AI coding army. Pure Rust single binary (~3.7 MB) that generates production-grade code using a 9-stage quality pipeline with TDD enforcement, multi-file extraction, surgical fix-pass retry, and a 9.2/10 quality gate.

## Quick Start

```bash
cargo build --release
./target/release/battlecommand-forge status          # check system
./target/release/battlecommand-forge mission "..."    # run a mission
./target/release/battlecommand-forge tui              # interactive TUI
```

### Remote Ollama (cloud GPU)

```bash
# On H200/cloud instance:
bash scripts/h200-setup.sh

# From your Mac:
OLLAMA_HOST=<ip>:11434 ./target/release/battlecommand-forge mission "..." --preset premium
```

## Architecture

### Dream Team v3 Pipeline (current best)

Grok architect + Opus tester + local 80B coder + Sonnet reviewers. Auth C7 passed 9.0 gate (65/67 tests, 97% pass rate).

| Role | Model | Cost | Speed | Why |
|------|-------|:---:|:---:|-----|
| **Architect** | grok-4.20-reasoning | ~$0.10 | xAI | Concise specs (107 lines), lean output. Reasoning model for complex designs. |
| **Tester** | claude-opus-4-6 | ~$0.20 | Anthropic | Correct pytest-asyncio fixtures, Pydantic v2, session isolation. 97% pass rate. |
| **Coder** | qwen3-coder-next:q8_0 | $0 | 26 tok/s | 80B, 64K context, always single-shot. C7+ auto-upgrades to Sonnet. |
| **Fix Coder** | claude-sonnet-4-6 | ~$0.05 | Anthropic | Precise surgical fixes for broken files. |
| **Security** | claude-sonnet-4-6 | ~$0.05 | Anthropic | OWASP review, honest scoring. |
| **Critique** | claude-sonnet-4-6 | ~$0.05 | Anthropic | 5-in-1 scoring (DEV/ARCH/TEST/SEC/DOCS). |
| **CTO** | claude-sonnet-4-6 | ~$0.05 | Anthropic | Mission-level coherence check. |
| **Complexity** | claude-sonnet-4-6 | ~$0.02 | Anthropic | Fast C1-C10 scoring. |

**Cost per mission: ~$0.30-0.50** (Grok architect + Opus tester + Sonnet reviews). Local coder = $0.

#### Claude Opus 4.6 API Pricing

| | Input | Output | Batch (50% off) |
|---|:---:|:---:|:---:|
| **Standard** | $5/MTok | $25/MTok | $2.50/$12.50 |
| **Cache write (5min)** | $6.25/MTok | — | — |
| **Cache hit** | $0.50/MTok | — | — |

Full 1M context window at standard pricing. No premium for long context.

#### Opus Tester vs Local 32B Tester

| | Local 32B Tester | Opus Tester |
|---|:---:|:---:|
| Tests passing (round 1) | 0/1 | **24/6** |
| Verifier score | 6.3 | **9.5** |
| Final score | 7.3 | **9.2** |
| Tester time | 253s | **55s** |
| Rounds to pass | 3+ (never) | **1** |
| Total pipeline time | 1276s+ | **510s** |
| Cost | $0 | ~$0.20 |

### 10-Mission Stress Test Results (March 2026)

All-local pipeline (no cloud API), `--auto` mode, up to 5 fix rounds per mission:

| # | Mission | Score | Files | LOC | Time | Verdict |
|---|---------|:-----:|:-----:|:---:|:----:|---------|
| 1 | C4 Simple CRUD API | **8.0** | 10 | 395 | 11m | ALMOST — dual Base bug, 5 tests pass |
| 2 | C3 CSV-to-JSON CLI | **7.9** | 11 | 456 | 16m | ALMOST — 9/10 tests pass, tool works |
| 3 | C8 Auth+JWT+RBAC | **7.9** | 44 | 2172 | 49m | NEEDS WORK — import errors in tests |
| 4 | C7 WebSocket Chat | 7.1 | 18 | 960 | 21m | NEEDS WORK — wrong router import |
| 5 | C5 Todo CRUD | 7.1 | 13 | 745 | 34m | NEEDS WORK — conftest syntax error |
| 6 | C4 File Monitor | **7.5** | 9 | 475 | 14m | ALMOST — production code works, 3 tests pass |
| 7 | C5 URL Shortener | **7.4** | 19 | 765 | 15m | SOME — 1 test passes |
| 8 | C6 Config Parser Lib | **8.1** | 13 | 745 | 25m | ALMOST — library works, CLI missing import |
| 9 | C9 E-commerce API | 7.1 | 30 | 1786 | 44m | NEEDS WORK — missing schema in tests |
| 10 | C2 Health Check | 6.5 | 8 | 242 | 14m | NEEDS WORK — conftest can't find app |

**Average score: 7.5/10 | Total: 4 hours | 5 of 10 have tests running**

Key findings:
- Production code is consistently 85-90% correct across all task types
- Test code is the weak point: wrong mock targets, Pydantic v1 assertions, missing imports
- Simpler tasks (CLI tools, scripts, libraries) score higher than complex APIs
- Single-shot generation (C1-C7) produces cleaner code than staged generation (C8+)
- A developer can ship any of the "ALMOST" projects in 15-20 minutes of fixes

### Pipeline (9 stages, enforced)

1. **Router** — dual assessment: rule-based keywords + AI complexity scoring (C1-C10)
2. **Architect** — ADR + file manifest + TDD test plan (32B, concise specs)
3. **Tester-first** — complete test suite written BEFORE implementation (32B, live streaming)
4. **Coder** — implements against tests, outputs multi-file project (80B, 128K context, always single-shot)
5. **Verifier** — creates venv, pip installs deps, runs ruff/pytest, checks secrets/TODOs
6. **Security Auditor** — OWASP Top 10 review
7. **Critique Panel** — 5 scores in single LLM call: DEV/ARCH/TEST/SEC/DOCS
8. **CTO Review** — mission-level coherence check
9. **Quality Gate** — ships only if `critique_avg * 0.4 + verifier * 0.6 >= 9.2`

### Fix-Pass Loop (surgical-only, no full regen)

Stages 4-8 retry up to 5 times. **Surgical fix only** — if broken files can't be identified, the round is skipped (no full regen, which always degrades quality).

**Surgical fix**: Identifies broken files via import chain tracing (NameError, AttributeError, TypeError, ImportError). Each broken file gets its own LLM call with file content + specific findings. Clean files stay untouched.

**Smart stopping**: If score declines for 2+ consecutive rounds, loop breaks early and restores best round's files to disk.

**Feedback rules**: "Fix ONLY bugs. Do NOT add features, middleware, auth, or rate limiting unless the original prompt asked for them." This prevents the coder from breaking working code to satisfy reviewer feature requests.

**Feedback limits** (scaled by complexity):
- C1-C7: verifier 800 chars, critique 400 chars, verdicts 200 chars, 10 test errors
- C8+: verifier 1500 chars, critique 600 chars, verdicts 400 chars, 20 test errors

### Code Generation (Always Single-Shot)

The coder generates ALL files in a single LLM call with 128K context. This eliminates inter-stage seam failures (missing imports, phantom fields, wrong module paths) that plagued the old 5-stage approach.

5-stage generation is preserved but unreachable — available for future C10+ mega-projects that exceed 128K token budget.

### Claude vs Local Comparison (C8 Auth Service)

| Metric | All-Claude Opus ($2-5) | Local 80B ($0.00) | Opus spec+test ($0.30) |
|--------|:-------------------:|:-----------------:|:---:|
| Tests passing | 82/144 (57%) | 0 → fixable | **24/30 (80%)** |
| Crash bugs | 0 | 6 (4 fixes) | 0 |
| Time to ship | 30 min | 30 min | **passed 9.2 gate** |
| Files / LOC | 61 / 5331 | 35 / 1569 | 16 / 1114 |

**Best ROI: Opus architect + Opus tester + local coder/reviewers** = ~$0.30/mission, passes quality gate on round 1. Each pipeline role can use a different model (local or cloud).

### Quality Guardrails

- **30+ known-bad patterns**: Language-specific warnings (Pydantic v2, MISRA naming, circular imports, DI wiring, schema/ORM naming collisions, httpx ASGITransport, mock targets, etc.)
- **`__init__.py` sanitization**: Auto-strips re-exports that cause circular imports
- **Dynamic failure memory**: `.battlecommand/failure_patterns.md` captures errors from failed runs, injects into future prompts
- **Import chain tracing**: NameError/AttributeError/TypeError → traces broken module → surgical fix targets exact file
- **Reasoning leak detection**: Rejects surgical fix output that contains LLM "thinking" text instead of code
- **pytest parser**: Safe number extraction (won't misread port numbers as test counts)
- **Best-round restore**: Ships highest-scoring round, not degraded last round

### Best Run Results (3-Model Pipeline)

The 3-model pipeline produced the best output ever — features never seen in any single-model run:

- **CSRF protection** — full token generation + validation + cookie management
- **Refresh token versioning** — server-side revocation via `refresh_token_version` column
- **Repository pattern** — `UserRepository` separates DB access from business logic
- **Alembic migrations** — real database migration system
- **Property-based tests** — `tests/property/test_auth_flows.py`
- **Async SQLAlchemy** throughout — `AsyncSession`, async repositories
- **HTTPSRedirectMiddleware** + **TrustedHostMiddleware** — production security
- **Custom exception hierarchy** — `CredentialsException`, `TokenExpiredError`, `RateLimitExceeded`, `CSRFTokenMismatch`
- **41 Python files** — most comprehensive output of all runs

### Model Benchmark Results (8+ models tested)

| Model | Size | Speed | Best Score | Notes |
|-------|:---:|:---:|:---:|-------|
| qwen2.5-coder:32b Q4 | 18.5 GB | 50 tok/s | **8.1** | Champion coder — SecretStr, DI, consistent |
| qwen3-coder-next:80B q8 | 79 GB | 27 tok/s | 7.6 | Best architect — CTO conditional approve |
| devstral-small-2:24b q8 | 24 GB | 17 tok/s | 7.6 | Security headers, Dev=9.5 |
| qwen3-coder:30b-a3b q8 | 30 GB | 47 tok/s | 7.1 | Fastest pipeline, most honest critic |
| deepseek-coder-v2 | 8.9 GB | 80 tok/s | 6.8 | Fastest but hardcoded secrets |
| glm-4.7-flash:q8_0 | 30 GB | 15 tok/s | 6.6 | Returns empty on generate API — unusable |
| qwen3.5:35b-a3b q8 (MoE) | 36 GB | 13 tok/s | — | Returns empty on surgical fixes — unreliable |
| qwen3.5:9b q8 | 10 GB | 18 tok/s | — | Burns all tokens on \<think\> tags — unusable without /no_think |
| nemotron-3-super | 81 GB | 23 tok/s | — | Great architect, verbose CTO (meta-reasons instead of answering) |
| nemotron-3-nano | 24 GB | Q4 | — | Same meta-reasoning problem, defaults to 7.0 scores |

### Key Learnings (from v2 + our benchmarks)

1. **Decompose quality cascades** — 80B architect spec → 32B coder produces CSRF, repo pattern, Alembic. 7B architect spec → 32B coder produces toy code with no database.
2. **Local models self-inflate scores** — qwen80b claims 8.5 but honest assessment is 7.2 (+1.3 pts bias). devstral gave 9.3 for broken code.
3. **qwen3-coder:30b is the most honest critic** — DEV:3, SEC:1 for mediocre code. No inflation.
4. **VRAM efficiency > parameter count** — 30B MoE beats 123B on speed + reliability.
5. **Single critique call > 5 parallel calls** — Ollama is sequential anyway. 1 call (~15s) vs 5 calls (~75s).
6. **Venv isolation required** — system Pydantic v1 + Python 3.12 = `ForwardRef` crash. Venv per project fixes it.
7. **Output caps prevent garbage** — `num_predict` limits stop models from rambling or repeating.
8. **Surgical fixes > full regeneration** — Fixing only broken files preserves clean code and uses ~70% less context.
9. **tool_exists() must handle absolute paths** — venv python is an absolute path, not in PATH. `which` fails on it, silently breaking all pip installs in the venv. Root cause of tests never running.
10. **Poetry pyproject.toml needs direct dep parsing** — `pip install -e .` fails on Poetry format. Must extract dep names and pip-install individually.
11. **Circular imports: routes ↔ dependencies** — LLMs consistently generate circular imports between route and dependency modules. Must be explicitly forbidden in prompts.
12. **Full regen always degrades score** — Surgical fix preserves good code; full regen throws it away and produces worse output. Always prefer surgical.
13. **Always single-shot with 128K context** — 5-stage generation caused 7 crash bugs on C8 auth (inter-stage seam failures). Single-shot with 128K context reduced to 1 systematic bug. The 80B model supports 256K native context.
14. **80B coder > 32B coder in practice** — 32b scored 8.1 in isolated benchmarks but averaged 6.8 in the 10-mission suite. 80B averaged 7.5 (+0.7). The larger context produces better multi-file coherence.
15. **32B is the optimal architect** — Tested 6 models across 10 prompts. 32B produces concisest specs (77 lines avg, 6 files avg) with zero overengineering. 80B was slowest (43s avg) and overspecified.
16. **Dual Base is the #1 recurring bug** — LLMs generate `Base = declarative_base()` in database.py AND `class Base(DeclarativeBase)` in models.py. Must be explicitly forbidden.
17. **Schema/ORM naming collision** — LLMs name both ORM model and Pydantic schema `User`, causing shadowing. Must use `UserResponse`/`UserRead` for schemas.
18. **Test code is the weak point** — Production code is 85-90% correct. Tests fail due to wrong mock targets, Pydantic v1 assertions, missing imports, async/sync mismatches.
19. **Best-round restore is essential** — Fix rounds often degrade scores. Pipeline restores best round's files instead of shipping degraded last round.
20. **Fix rounds must fix bugs, not add features** — Reviewers ask for auth/rate-limiting/CSRF, coder adds them, breaks working code. Feedback now says "fix ONLY bugs."
21. **Scoring: tests > opinions** — Old formula (critique 70%) was unreachable. New formula (verifier 60%) rewards working tested code.
22. **Nemotron models meta-reason** — nemotron-nano outputs "We need to output a verdict..." instead of the actual verdict. Not usable for critique/CTO.
23. **qwen3.5 models need /no_think** — Without it, they burn entire output budget on \<think\> reasoning tags.
24. **GLM returns empty via generate API** — Always defaults to 7.0 scores. Not usable for critique.
25. **MoE models unreliable for surgical fixes** — qwen3.5:35b (3B active) returns empty on targeted edit prompts.
26. **Opus tester > local 32B tester** — Local 32B writes tests that never run (wrong imports, missing fixtures, wrong function signatures). Opus writes tests with correct pytest-asyncio patterns, dependency overrides, and session isolation. 24/30 pass immediately vs 0/1. Cost: ~$0.20 per mission. Saves 3+ failed fix rounds.
27. **Critique parser must strip markdown** — qwen3-coder:30b formats scores with markdown bold (`**DEV**: 7.0`). Parser must strip `*` and `#` before matching prefixes, or all scores default to fallback.
28. **codegen must sanitize paths** — LLM outputs `## app/__init__.py` as a header. Without stripping `#` from paths, a directory named `## app/` gets created. Also must strip inner code fences from config files (pyproject.toml wrapped in `` ```toml ``).
29. **64K coder context is sufficient** — C8 missions use ~40-45K tokens. 128K wastes VRAM on KV cache. 64K saves VRAM and speeds up inference.

### LLM Client

- **Claude API**: Used when model starts with `claude-`. Opus 4.6 pricing: $5/MTok input, $25/MTok output. Cache hits: $0.50/MTok.
- **Grok API**: Used when model starts with `grok-`. xAI OpenAI-compatible API at `api.x.ai`. Requires `XAI_API_KEY`.
- **Ollama**: Default for local models. Multi-model config with per-role context and output limits.
- **Mix-and-match**: Each role can use a different provider (xAI, Anthropic, Ollama). Best config: Grok architect + Opus tester + local coder + Sonnet reviews.
- **Remote Ollama**: Set `OLLAMA_HOST=host:port` to use a remote GPU instance.
- **Streaming**: `generate_live()` streams tokens to stdout in real-time (dim gray) for all providers (Ollama NDJSON, Claude SSE, Grok SSE).
- **Tool calling**: `chat_with_tools()` supports native tool calling for all 3 providers (Ollama `/api/chat`, Claude `tool_use`, Grok OpenAI functions). Text-based `TOOL_CALL:` fallback for models without native support.
- **Timeout**: 900 seconds.
- **Per-role limits**: Architect 4K output, Coder 32K output, Reviews 1K output.
- **Smart VRAM offloading**: Only offloads between stages when models are different.

### Router (Dual Assessment)

Ported from battleclaw-v2. Two-factor complexity scoring:
1. **Rule-based** — keyword tiers (trivial/moderate/high/extreme) + structural analysis + length + language modifier
2. **AI-assisted** — LLM scores complexity 1-10 with reasoning
3. **Blending** — if AI disagrees by 2+, uses weighted average or trusts higher-confidence source

### Multi-File Code Extraction

The Coder outputs multiple files with `### path/to/file.py` headers before code fences. `codegen.rs` parses these into individual files. Falls back to single-file extraction if no paths detected.

**No subtask decomposition** — removed because it caused duplicate project structures.

### Quality Scoring

```
final_score = critique_avg * 0.4 + verifier_score * 0.6
```

Critique panel: single LLM call returns DEV/ARCH/TEST/SEC/DOCS scores (0-10 each).

## Module Map (30 modules)

| Module | Purpose |
|--------|---------|
| `mission.rs` | 9-stage pipeline orchestration + surgical fix-pass loop + v2 context management |
| `tui.rs` | ratatui 6-tab TUI with CTO chat, scrolling, 15 slash commands, tool call display |
| `llm.rs` | Claude API + Ollama + Grok client + streaming + tool calling + OLLAMA_HOST |
| `cto.rs` | CTO chat agent with native tool calling (10 tools, 5 iterations, history persistence) |
| `verifier.rs` | Venv creation + pip install + ruff/pytest execution + secret detection |
| `codegen.rs` | Multi-file extraction from LLM output |
| `model_config.rs` | Per-role model config (preset → env → TOML → CLI resolution) |
| `model_picker.rs` | Interactive model selection UI overlay for TUI |
| `report.rs` | Pipeline report generation + JSON viewer |
| `router.rs` | Dual complexity scoring (rules + AI, ported from v2) |
| `editor.rs` | Edit existing codebases via LLM |
| `memory.rs` | Learnings + few-shot examples + context injection |
| `sandbox.rs` | Sandboxed execution, timeouts, env stripping, path validation, network sandbox |
| `models.rs` | Ollama model listing, benchmarking, VRAM estimation |
| `hardware.rs` | CPU/RAM/VRAM/Ollama monitoring for TUI |
| `enterprise.rs` | Audit logging, cost tracking, RBAC |
| `workspace.rs` | Isolated git workspaces per mission |
| `swebench.rs` | SWE-bench evaluation: ReAct agent loop, dataset handling, workspace management |
| `swebench_tools.rs` | 7 ReAct tools: read_file, grep_search, list_directory, run_command, write_file, apply_edit, submit |
| `swebench_eval.rs` | SWE-bench report generation with per-repo breakdown + baseline comparison |
| `benchmark.rs` | Multi-model benchmark framework (5 graded missions, full/quick phases) |
| `swarm.rs` | Swarm mode: planner→coder→QA iteration with best-version selection |
| `custom_commands.rs` | User-defined commands from `.battlecommand/commands/*.md` files |
| `stress.rs` | 21-task stress test suite (C4-C9) |
| `snake.rs` | Easter egg snake game (type /snake in TUI chat) |
| `space.rs` | Easter egg Space Invaders game (type /space in TUI chat) |
| `db.rs` | Mission history (JSON file-based) |
| `context.rs` | Context compaction at 95% capacity |
| `github.rs` | GitHub push/PR via gh CLI |
| `voice.rs` | macOS TTS announcements (say command) |
| `main.rs` | CLI entry point (clap) |

## CLI Commands

```
mission "prompt" [--preset fast|balanced|premium] [--voice] [--auto] [-o dir] [--repo url] [--path dir]
chat [--preset premium]
edit --path . "what to change"
verify --path output/project_dir [--lang python]
stress --tasks 21 [--preset premium]
tui
models list|benchmark <model>|presets
github check|push|create-pr
swebench run [--variant lite|verified|full] [--instance id] [--model model] [--resume]
swebench report
swebench list [--repo name]
benchmark [--phase full|quick] [--tasks 5]
swarm "prompt" [--iterations 3] [--preset premium] [--lang python] [-o dir]
init
status
audit --limit 20
settings show|init
report list|show
hw
```

### TUI Slash Commands

| Command | Action |
|---------|--------|
| `/mission <prompt>` | Launch a mission from chat |
| `/verify [path]` | Run verifier (default: latest output) |
| `/report [list\|show]` | View pipeline reports |
| `/audit [n]` | Show audit log (default: 10) |
| `/preset <name>` | Switch preset (fast/balanced/premium) |
| `/cost` | Show total API cost |
| `/settings` | Open model picker overlay |
| `/clear` | Clear chat display + CTO history |
| `/compress` | Compact CTO conversation history |
| `/models` | Switch to Models tab |
| `/hw` | Switch to Hardware tab |
| `/status` | Show workspace/system info |
| `/snake` | Play snake! |
| `/space` | Play Space Invaders! |
| `/help` | List all commands |

### TUI Key Bindings

| Key | Action |
|-----|--------|
| 1-6 | Switch tabs (when input empty) |
| Tab | Cycle tabs |
| PgUp/PgDn | Scroll chat ±20 lines |
| Up/Down | Scroll chat ±3 lines (input empty) |
| Home/End | Scroll top/bottom (input empty), cursor start/end (typing) |
| Left/Right | Move input cursor |
| Esc | Clear input or quit |

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `OLLAMA_HOST` | Remote Ollama URL (e.g. `192.168.1.100:11434` or `http://h200:11434`) |
| `ANTHROPIC_API_KEY` | Claude API key (enables Opus/Sonnet models) |
| `XAI_API_KEY` | xAI API key (enables Grok models) |
| `BRAVE_API_KEY` | Brave Search API key (CTO web search, falls back to DuckDuckGo) |
| `ARCHITECT_MODEL` | Override architect model |
| `CODER_MODEL` | Override coder model |
| `TESTER_MODEL` | Override tester model |
| `SECURITY_MODEL` | Override security model |
| `CRITIQUE_MODEL` | Override critique model |
| `CTO_MODEL` | Override CTO model |
| `REVIEWER_MODEL` | Override security + critique + CTO together |

## Key Design Decisions

1. **Dream team pipeline** — 80B architect + 32B coder + 30B honest critic + devstral security/CTO.
2. **No Python bridge** — Pure Rust. v2 had FastAPI/CrewAI; we killed it.
3. **No subtask decomposition** — Caused duplicate projects. Multi-file extraction is better.
4. **Quality gate at 9.2** — Intentionally high. Pushes models to produce production-grade code.
5. **Surgical fix-pass (v2-style)** — Fix only broken files, not full regeneration. 70% less context.
6. **Single critique call** — 5 scores in 1 LLM call (DEV/ARCH/TEST/SEC/DOCS), not 5 sequential calls.
7. **Venv per project** — Isolates dependencies, avoids Pydantic v1/v2 conflicts.
8. **Streaming by default** — Architect/Tester/Coder/Surgical fixes show tokens live.
9. **Output caps** — Architect 4K, Coder 16K, Reviews 1K. Prevents garbage/repetition.
10. **Remote Ollama support** — `OLLAMA_HOST` env var for cloud GPU inference.
11. **Native tool calling** — CTO uses Ollama `/api/chat` tools, Claude `tool_use`, or Grok functions. Text-based `TOOL_CALL:` fallback for models without native support.
12. **Secure sandbox** — Pattern-based env var stripping, subprocess timeouts (kill on deadline), path traversal validation, macOS `sandbox-exec` for network-denied test execution.
13. **Configurable output** — `--output` for custom dir, `--repo`/`--path` for existing codebase context injection.

## TUI Polish (v2 Parity) — DONE

All v2 TUI features have been ported to forge:

- [x] **Code tab typewriter effect** — 12 chars/tick (~240 chars/sec) with safe char boundary handling and blinking block cursor.
- [x] **Thinking visualization** — Live LLM thinking/reasoning in Log tab top section. ThinkingChunk events from pipeline, shows `[model] thinking...` / `[model] done`.
- [x] **Status bar** — Rich bar: `FORGE` badge (red bg) | status (READY/STREAMING/MISSION) | task counter `[completed/total]` | live cost `$0.0000` | VRAM usage | help text.
- [x] **Live cost display** — `CostUpdate` event in TuiEvent enum. Running total in status bar.
- [x] **Scroll pinning on Code + Log tabs** — PgUp/Up pins scroll, PgDn/Down/End resumes auto-scroll. Home jumps to top. Matches Chat tab behavior.
- [x] **Chat message model labels** — `[YOU]`, `[model-name]`, `[SYS]`, `[TOOL]`, `[ERR]` prefixes with bold styling. Streaming shows `[model ...]`.
- [x] **Duplicate mission guard** — `mission_running` flag blocks concurrent `/mission` launches.
- [x] **Header HW summary** — CPU/RAM/VRAM displayed in tab bar title. Always polled (not just on HW tab).
- [x] **User message truncation** — Messages >80 chars truncated to 77 + `... [N chars]`.
- [x] **Status line updates** — READY / CTO STREAMING [model]... / Stage: X [model] / MISSION COMPLETE / MISSION FAILED.
- [x] **/clear clears thinking buffer** — Thinking visualization reset along with chat history.

## Building

```bash
cargo build --release   # ~3.7 MB binary (LTO + strip)
cargo test              # unit tests (86 tests)
```

Requires: Rust 1.75+, Ollama running (`ollama serve`), optionally `ANTHROPIC_API_KEY`.
