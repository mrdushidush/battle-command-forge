# BattleCommand Forge -- 2-Minute Demo Video Script

## Pre-Recording Setup

- Terminal: dark theme (Dracula or similar), font size 16+, 120 columns wide
- Have `battlecommand-forge` built and in PATH
- Ollama running with `qwen3-coder-next:q8_0` pulled
- `ANTHROPIC_API_KEY` and `XAI_API_KEY` set
- Clean output directory (no leftover missions)
- Screen recording at 1920x1080
- Microphone: clear, close, no room echo

---

## [0:00-0:10] Hook

**On screen:** Black terminal, cursor blinking. Type the command slowly enough for viewers to read.

**Type:**
```
battlecommand-forge mission "Build a FastAPI auth service with JWT and role-based access control"
```

**Narrator:** "Every AI coding tool generates code and hopes for the best. This one has a 9-stage quality pipeline. Watch."

**Text overlay (bottom-right, subtle):** `battlecommand.dev`

**Hit Enter. The pipeline starts streaming.**

---

## [0:10-0:30] The Problem

**On screen:** While the Router stage runs (takes ~3 seconds), show a quick split-screen or text overlay sequence. If editing is too complex, just narrate over the pipeline starting.

**Narrator:** "Cursor generates code but doesn't test it. Copilot suggests lines but doesn't verify them. Devin costs five hundred dollars a month. None of them have a quality gate -- a threshold your code must pass before it ships."

**On screen:** The Router output appears:
```
[Router] Complexity: C8 (high) -- dual assessment
```

**Narrator:** "BattleCommand Forge scores complexity first, then runs your code through nine stages."

**Text overlay (brief, 2 seconds):** `9 stages. Quality gate at 9.2/10.`

---

## [0:30-0:50] Architect + Tester

**On screen:** Architect stage streams tokens live. Dim gray text flowing across the screen.

**Narrator:** "Stage two: the Architect writes a design doc -- file manifest, API contracts, database schema. This isn't a suggestion. It's a spec the coder must implement against."

**On screen:** Architect completes. Tester stage begins. Highlight or callout the word "Tester" when it appears.

**Narrator:** "Stage three: tests are written BEFORE code. Not after. Before. Thirty tests defining exactly what the auth service must do. The coder generates against this test suite."

**Text overlay (brief):** `TDD: Tests written FIRST`

---

## [0:50-1:10] Coder

**On screen:** Coder stage streams. Show the file headers appearing: `### app/main.py`, `### app/models.py`, `### app/routers/auth.py`, etc.

**Narrator:** "The coder generates all files in a single shot -- sixteen files, eleven hundred lines. It's an eighty-billion parameter model running locally on Ollama. Cost: zero."

**On screen:** Let the streaming run for a few seconds. Viewers should see real code flowing.

**Text overlay (brief):** `80B model, local, $0`

---

## [1:10-1:30] Verification + Reviews

**On screen:** Verifier stage starts. Show the venv creation and pytest output.

```
[Verifier] Creating venv... installing deps...
[Verifier] pytest: 24/30 passed (80%)
[Verifier] ruff: 0 errors
```

**Narrator:** "Stage five: the verifier creates a virtual environment, installs dependencies, runs ruff for linting, and runs pytest. Twenty-four of thirty tests pass on the first round."

**On screen:** Security, Critique, and CTO stages flash by.

```
[Security]  OWASP review: no critical findings
[Critique]  DEV:9 ARCH:9 TEST:8 SEC:9 DOCS:8
[CTO]       Approved
[Gate]      Score: 9.2/10 -- SHIPPED
```

**Narrator:** "Then: OWASP security audit. Five-score critique panel. CTO review. And the quality gate. If the score is below nine-point-two, it goes back for surgical fixes -- tracing broken imports, fixing only the files that failed. No full regeneration."

**Text overlay (brief):** `Quality Gate: 9.2/10`

---

## [1:30-1:50] The Numbers

**On screen:** Run the status command (or show pre-prepared output):

```
$ battlecommand-forge status
```

Show output with key stats. If status doesn't show all these, prepare a terminal with the numbers visible.

**Narrator:** "Thirty Rust modules. Single binary, three-point-seven megabytes. No Python, no Node, no runtime dependencies. Mixes local and cloud models -- the coder runs free on Ollama, architect on Grok, tester on Claude Opus, reviews on Sonnet. Total cost for this mission: thirty-one cents."

**On screen:** Show the output directory briefly:

```
$ ls output/auth_service/
app/  tests/  requirements.txt  pyproject.toml  alembic/  alembic.ini
```

**Narrator:** "CSRF protection. Refresh token versioning. Repository pattern. Alembic migrations. All generated, all tested."

---

## [1:50-2:00] CTA

**On screen:** Clear terminal. Show the install command or GitHub URL prominently:

```
cargo install battlecommand-forge
```

Or:

```
https://github.com/mrdushidush/battle-command-forge
```

**Narrator:** "BattleCommand Forge. Quality-first AI coding. The first tool that won't ship your code unless it's good enough. Link in the description."

**Text overlay (centered, large, hold for 5 seconds):**
```
battlecommand.dev
First 1,000 founding members: $15 lifetime v1.x updates
```

**Fade to black.**

---

## Post-Production Notes

- **Total runtime target:** 1:55-2:05. Cut narration pauses, not content.
- **Do NOT speed up the terminal.** Real-time streaming is the demo. Viewers need to see tokens flowing.
- **Music:** None, or extremely subtle ambient. Terminal audio is part of the experience.
- **If a stage takes too long:** Pre-record each stage separately, splice at stage boundaries. The viewer won't notice cuts between `[Stage]` lines.
- **Fallback:** If the mission doesn't hit 9.2 on camera, use `--preset premium` (Sonnet coder) which passes more reliably. Or record multiple takes and use the best one.
- **Resolution:** Export at 1080p minimum. Terminal text must be readable on a phone screen.
- **Thumbnail:** Screenshot of the quality gate line: `[Gate] Score: 9.2/10 -- SHIPPED` with green text.
