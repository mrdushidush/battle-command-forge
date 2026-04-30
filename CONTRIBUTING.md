# Contributing to BattleCommand Forge

Thanks for taking an interest. BCF is a solo-maintainer project;
contributions are welcome but reviewed as time allows — please be
patient, and don't treat a delayed response as disinterest.

## Before you start

**File an issue first for anything non-trivial.** A two-line issue
saves us both from a 500-line PR that doesn't fit the project's
direction. Bug reports with a reproducer are always welcome without
prior discussion; feature proposals work best as an issue first.

**What BCF is:** a quality-first AI code-generation pipeline in pure
Rust. Single binary, 9-stage TDD pipeline (router → architect →
tester → coder → verifier → security → critique → CTO → quality
gate), surgical fix-pass retry, multi-provider LLM client (Ollama
local + Anthropic Claude + xAI Grok). See the
[`README.md`](README.md) for the user-facing tour and
[`CLAUDE.md`](CLAUDE.md) for pipeline internals.

**What BCF isn't going to become:** a hosted SaaS, a SaaS-style
multi-cloud abstraction, a VS Code extension, or a generic agent
framework. Proposals in those directions will be politely declined —
the whole point is to stay small, local, and pipeline-focused.

## Development setup

```bash
git clone https://github.com/mrdushidush/battle-command-forge
cd battle-command-forge
cargo build --release
```

You'll need Ollama running locally for any end-to-end testing of the
default (all-local) preset, plus optional `ANTHROPIC_API_KEY` /
`XAI_API_KEY` for testing cloud-API roles. See
[`CLAUDE.md`](CLAUDE.md) for the recommended hardware profile.

## Before you open a PR

Run these checks. They're the same ones CI runs, so if they're green
locally, CI will be green too:

```bash
cargo fmt --all --check
cargo clippy --all-targets --no-deps --locked -- -D warnings
cargo test --lib --bins --locked
```

All must pass. Tests currently sit at **98 lib + 0 bin passing** —
a PR that drops the pass count needs a justification in the
description.

## Commit style

[Conventional Commits](https://www.conventionalcommits.org/). Every
commit on `main` uses one of these prefixes:

- `feat:` — new user-visible functionality
- `fix:` — bug fixes
- `refactor:` — internal reorganisation, no behavioural change
- `docs:` — README / CHANGELOG / `docs/*` edits
- `test:` — test-only changes
- `style:` — formatting only (`cargo fmt`)
- `chore:` — release prep, dep bumps, housekeeping
- `ci:` — changes under `.github/workflows/`

Keep the first line under 72 chars; prose in the body is encouraged
when the WHY is non-obvious. Look at `git log` for examples — the
existing history is the style guide.

## Adding a new pipeline stage or model role

The 9-stage pipeline is defined in `src/mission.rs::MissionRunner`
and the per-role model resolution is in `src/model_config.rs`. The
shape of a typical change:

1. Decide whether you're adding a new **stage** (a new step in the
   sequential pipeline) or a new **role** (a new model alongside the
   existing architect/tester/coder/security/critique/CTO/complexity
   roles). Stages are rarer; most contributions are role tweaks or
   prompt revisions.
2. For a new role: add a `RoleConfig` slot in
   `src/model_config.rs`, plumb it through the preset definitions
   (fast/balanced/premium), and add the env-var override constant
   (e.g. `MY_NEW_MODEL`).
3. For a new stage: extend `MissionRunner::run` with the new step
   between the existing ones; emit `TuiEvent::StageStarted` so the
   TUI status line updates; thread feedback into the round report
   builder.
4. Add at least one unit test covering the happy path and one
   covering a known failure mode (model returns empty, network
   times out, parse error).

Document the change in the README's CLI / preset / TUI section so
users can discover it.

## Adding a new CTO chat tool

The CTO agent in `src/cto.rs` exposes 10 tools to the model via
native tool-calling. To add an 11th:

1. Add a JSON schema entry to `build_tools()` in `src/cto.rs`.
2. Add a handler arm to `CtoAgent::execute_tool` matching the new
   tool name.
3. **Network-touching tools must wrap their return value in
   `<untrusted source="...">…</untrusted>` per the threat model
   in `SECURITY.md`.** The `web_search` and `web_fetch` helpers are
   the working examples.
4. Add a unit test in the existing `mod tests` block at the bottom
   of `cto.rs`.

## Adding a new SWE-bench `run_command` head

The argv allowlist in
`src/swebench_tools.rs::ALLOWED_RUN_COMMAND_HEADS` is intentionally
tight — every entry is a deliberate security decision. To add one:

1. Open an issue first explaining the use case (which SWE-bench
   workflow needs the new head).
2. If approved, add the head to `ALLOWED_RUN_COMMAND_HEADS` and add
   a test in the `mod tests` block at the bottom of the file
   covering the new head.
3. If the head requires shell composition (`&&`, `|`, redirects),
   propose a higher-level tool instead (e.g.
   `pytest_with_coverage`) — composing through `sh -c` is rejected
   by design.

## Testing guidelines

- Tests that mutate environment variables must wrap their access in
  `unsafe { std::env::set_var(...) }` and clean up in
  `unsafe { std::env::remove_var(...) }`. Future Rust editions may
  enforce serialization; for now, prefer not to run tests in
  parallel that touch the same env-var.
- Fixture-based tests should keep fixtures under
  `tests/fixtures/<area>/` rather than inlining 5 KB of JSON into a
  `#[test]`.
- Live tests (tests that hit the real Ollama / Anthropic / xAI) go
  behind `#[ignore]` with a doc comment explaining what env vars or
  credentials are needed. Never in CI; run manually with
  `cargo test -- --ignored`.

## Reporting bugs

File at <https://github.com/mrdushidush/battle-command-forge/issues>
with:

1. What you did — the exact mission prompt or CLI command.
2. What you expected.
3. What actually happened — full error output if there is one.
4. Your setup — OS, Ollama version, model names, preset, complexity
   band reported by the router.

A minimal reproducer is worth more than a paragraph of description.

## License

BCF is licensed under Apache-2.0. By contributing, you agree that
your contributions are licensed under the same terms. No CLA, no
copyright assignment — just the implicit licence grant from the
license file.
