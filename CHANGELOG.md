# Changelog

All notable changes to battlecommand-forge are documented here. The format loosely follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-04-23

Initial public release. This is a port of internal pipeline work that was developed and field-tested in a private repository from January through April 2026. Shipping as **v0.1.0** to honestly signal "stable but API may not be stable" — the code itself is proven (86 unit tests, 10-mission stress suite averaged 7.5/10 on all-local pipeline, dream-team pipeline hits 9.2+ gate consistently on C7-class work), but this is the first public surface and refinements may land without deprecation windows until v1.0.

### Added

- **9-stage quality pipeline** (`mission.rs`): router → architect → tester → coder → verifier → security → critique → CTO → quality gate. Ships only when `critique_avg * 0.4 + verifier_score * 0.6 >= 9.2`.
- **Dream-team pipeline preset**: Grok-4 architect + Claude Opus tester + local 80B coder (qwen3-coder-next:q8_0) + Claude Sonnet reviewers. ~$0.30–0.50 per mission. Passes gate round-1 on C7-class auth-service missions.
- **Surgical fix-pass retry**: Up to 5 rounds of targeted fixes on only the files with failing imports / tests. Best-round restore on degradation. No full regeneration (which historically tanked quality).
- **Dual-assessment complexity router** (`router.rs`): rule-based keyword + structural scoring blended with AI-assisted 1–10 rating, with disagreement-blending logic.
- **Multi-file codegen** (`codegen.rs`): parses `### path/to/file` headers from LLM output into individual files; sanitizes paths, strips inner code fences, rejects reasoning-leak output.
- **Three-provider LLM client** (`llm.rs`): Anthropic (Claude), xAI (Grok OpenAI-compatible), Ollama (local + remote via `OLLAMA_HOST`). Live streaming for all three. Native tool-calling for all three with text-based `TOOL_CALL:` fallback.
- **TUI** (`tui.rs`): ratatui-based 6-tab interface (Chat, Code, Log, Hardware, Models, Workspace), 15 slash commands, live cost tracking, CTO chat with tool calling (read_file, grep_search, run_command, web_search, etc.), typewriter effect on code-tab output.
- **Sandboxed verifier** (`verifier.rs`): per-project venv creation, pip install, ruff/pytest execution with subprocess timeouts, pattern-based env-var stripping, path-traversal validation, macOS `sandbox-exec` network denial.
- **Benchmark framework** (`benchmark.rs`): 5 graded missions across configurable model presets for A/B comparison.
- **SWE-bench integration** (`swebench.rs`, `swebench_tools.rs`, `swebench_eval.rs`): ReAct agent loop with 7 tools over SWE-bench lite/verified/full datasets, per-repo breakdown + baseline comparison in reports.
- **Swarm mode** (`swarm.rs`): planner → coder → QA iteration with best-version selection across N parallel runs.
- **30+ quality guardrails** hard-coded into the pipeline:
  - Dual-Base SQLAlchemy bug prevention
  - Schema/ORM naming collision prevention (`UserResponse` vs `User`)
  - Circular imports routes ↔ dependencies
  - `__init__.py` re-export stripping
  - Pydantic v2 pattern enforcement
  - Dynamic failure-pattern memory from past runs injected into future prompts
- **Per-role model overrides** via env vars (`ARCHITECT_MODEL`, `CODER_MODEL`, `TESTER_MODEL`, `SECURITY_MODEL`, `CRITIQUE_MODEL`, `CTO_MODEL`, `REVIEWER_MODEL`).
- **Configurable quality gate**, preset system (`fast`/`balanced`/`premium`), voice announcements on macOS (`voice.rs`).
- **Easter eggs**: `/snake` and `/space` (Space Invaders) playable in the TUI chat tab.

### Documented

- **CLAUDE.md** — developer guide with pipeline internals, model benchmark tables, 29 numbered learnings from the internal development period, TUI polish history, key design decisions.
- **BMORE.md** — extended architecture notes.
- **site/SHOW-HN.md** — draft HN submission when/if a public launch is staged.
- **site/DEMO-SCRIPT.md** — demo walkthrough script.

### Known Limitations

- Test code quality is the weak point across all models surveyed (~85–90% of production code is correct on first try; tests more frequently fail due to wrong mock targets, Pydantic v1 assertions, or missing imports). Learning #18 in CLAUDE.md.
- Local 32B models self-inflate critique scores by ~1.3 points vs honest assessment. Learning #2. Use the honest-critic model (`qwen3-coder:30b`) or Claude Sonnet for critique if score accuracy matters.
- MoE models (qwen3.5:35b-a3b) return empty output on surgical fix prompts — unreliable for fix rounds. Learning #25.
- Opus/Sonnet/Grok usage requires `ANTHROPIC_API_KEY` / `XAI_API_KEY`. All-local (Ollama-only) works but scores 1–1.5 points lower on average.
- Windows is untested for the pipeline stages that invoke venv/pytest; development and testing happened on macOS + Linux with remote Ollama.

### License

Apache-2.0. Prior internal releases used a proprietary license; the public release is relicensed for open community contribution.

[0.1.0]: https://github.com/mrdushidush/battle-command-forge/releases/tag/v0.1.0
