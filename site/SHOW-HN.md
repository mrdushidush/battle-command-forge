# Show HN: BattleCommand Forge -- AI coding tool with a 9-stage quality pipeline

Every AI coding tool I've used follows the same pattern: generate code, dump it in a file, hope it works. No tests. No security review. No quality threshold. You're the QA department.

I built BattleCommand Forge to fix this. It's a single Rust binary (3.7 MB, no runtime deps) that runs every code generation through a 9-stage pipeline: complexity routing, architecture spec, test-first TDD, code generation, verification (venv + pytest + ruff), OWASP security audit, critique panel (5 scores), CTO review, and a quality gate. Code doesn't ship unless it scores 9.2/10.

Here's what a mission looks like:

```
$ battlecommand-forge mission "Build a FastAPI auth service with JWT, RBAC, and refresh tokens"

[Router]    Complexity: C8 (high) -- dual assessment: rules + AI
[Architect] Writing ADR + file manifest + test plan... (streaming)
[Tester]    Writing 30 tests BEFORE implementation...
[Coder]     Generating 16 files against test suite... (80B local model)
[Verifier]  Creating venv... installing deps... running pytest: 24/30 passed
[Security]  OWASP Top 10 review... no critical findings
[Critique]  DEV:9 ARCH:9 TEST:8 SEC:9 DOCS:8
[CTO]       Approved with minor suggestions
[Gate]      Score: 9.2/10 -- SHIPPED

Output: ./output/auth_service/ (16 files, 1114 LOC)
Cost: $0.31 | Time: 8m 30s
```

The key insight: tests are written BEFORE code. The coder generates against a test suite, not a vague prompt. When tests fail, a surgical fix loop traces import chains to identify exactly which files are broken and fixes only those -- no full regeneration.

**Real numbers from a 10-mission stress test** (C2-C9 complexity, all automated):
- Average score: 7.5/10
- Production code: 85-90% correct across all missions
- 5/10 had tests passing immediately, rest needed minor fixes
- Cost: $0.30-0.50 per mission
- Best result: C8 Auth Service with CSRF protection, refresh token versioning, repository pattern, Alembic migrations, 24/30 tests passing first round

It mixes local and cloud models per pipeline role. The coder runs on a local 80B model via Ollama ($0). Architect uses Grok. Tester uses Claude Opus. Reviews use Sonnet. You can override any role.

**Tech:** Pure Rust, 30 modules, ~14K LOC. Streams tokens live. Runs on Mac/Linux. Supports remote Ollama for cloud GPU inference. TUI with 6 tabs and a CTO chat agent that has tool calling.

This is the only tool I know of that won't ship your code unless it passes a quality gate. Most of the time it doesn't pass -- and that's the point. It keeps iterating until it does.

GitHub: https://github.com/mrdushidush/battle-command-forge

Happy to answer questions about the pipeline, model selection, or why surgical fixes beat full regeneration.
