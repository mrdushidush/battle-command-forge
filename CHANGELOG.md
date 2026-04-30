# Changelog

All notable changes to battlecommand-forge are documented here. The format loosely follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] — 2026-04-30

Hygiene + crates.io debut. v0.1.0 was a public-source-drop; v0.2.0 is the
first release engineered for `cargo install battlecommand-forge`. Security
hardening across the LLM-controllable surfaces (the SWE-bench `run_command`
tool, path validation in the sandbox, env-var leakage to subprocess, and
prompt-injection wrapping on web tools), full CI/CD overhaul mirroring the
claudette v0.2.3 pattern, and a complete `.github/` contributor kit.

### Added

- **`secrets::write_secret_file` helper** — atomic temp-file + rename, with
  Unix mode 0600 set at create time. Used for `cto.rs` chat history
  persistence and for ensuring `.battlecommand/audit.jsonl` /
  `.battlecommand/costs.jsonl` are never written world-readable.
- **`.github/dependabot.yml`** — weekly cargo + GitHub-Actions dependency
  updates, with cargo minor/patch grouped into a single PR.
- **`.github/workflows/release.yml`** — tag-triggered crates.io publish via
  OIDC trusted-publisher (no `CARGO_REGISTRY_TOKEN` secret in the repo).
  Includes a `tag-version-match` job so a mistyped tag fails fast instead of
  publishing the wrong version.
- **MSRV CI verification job** — separate job that builds against the
  declared `rust-version = "1.95"` so the claim doesn't drift.
- **Multi-OS CI matrix** — clippy / test / build now run on Ubuntu, Windows,
  and macOS (was Ubuntu-only).
- **`.github/` contributor kit** — `PULL_REQUEST_TEMPLATE.md`,
  `ISSUE_TEMPLATE/{bug_report,feature_request,config}.md`. `blank_issues`
  disabled; security advisories direct users to GitHub's private flow.
- **Root contributor docs** — `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`,
  `SECURITY.md` with BCF-specific threat-model section covering the argv
  allowlist, path canonicalization, env allowlist, and `<untrusted>` web-tool
  boundary.
- **README badges** — CI status, crates.io version, license, MSRV.
- **CHANGELOG `[Unreleased]` block** — preserved between releases so future
  user-visible changes accumulate cleanly.

### Changed

- **`rust-version` corrected from 1.91 to 1.95.** The previous value was
  inaccurate — the codebase uses `is_multiple_of` (stable 1.95),
  `is_none_or` (1.82), and `std::path::absolute` (1.79). 1.95 is the real
  floor. Now CI-verified.
- **Cargo.toml description tone.** "9.2/10 quality gate" replaced with
  "complexity-scaled quality gate (up to 9.2/10)" to match the actual
  per-band thresholds (9.2 / 8.5 / 8.0 across C1-C6 / C7-C8 / C9-C10).
- **`tokio` features narrowed** from `["full"]` to
  `["rt-multi-thread", "macros", "sync", "process", "fs", "time"]`.
  Removes `mio` / `signal-hook-registry` / `socket2` from the dep tree.
- **`reqwest` switched to `rustls-tls`** with `default-features = false`.
  Eliminates the OpenSSL build dep; faster cross-compile, no `openssl-sys`
  in the lockfile.
- **`[profile.release]`** — added `panic = "abort"` (drops unwind tables) and
  `codegen-units = 1` (extra size shrink, marginal perf).
- **`Cargo.toml exclude`** — `site/`, `scripts/`, `BMORE.md`, `CLAUDE.md`,
  `.battlecommand/`, `.grok/` are no longer in the published tarball.
- **All third-party GitHub Actions are now SHA-pinned** with `# vX.Y.Z`
  comments (closes the tag-mutation supply-chain class). `dtolnay/rust-toolchain@stable`
  is intentionally left as a rolling alias.
- **Top-level workflow `permissions: contents: read`** (default-deny). Jobs
  that need more, like `cargo audit`, opt back in explicitly.
- **CI cargo cache** moved from a hand-rolled `actions/cache@v4` to
  `Swatinem/rust-cache@v2`.
- **CI uses `--locked`** on `cargo test`, `cargo clippy`, and `cargo build`.
- **`cargo fmt` invocation now uses `--all`** in CI (the old `cargo fmt --
  --check` skipped nested module trees on Windows).
- **README slash-command count** — line 292 said "14 slash commands";
  reconciled to the actual 15 documented in the table.

### Fixed

- **`mission.rs`: replaced `expect("BUG: no rounds completed")` with a
  `const _: () = assert!(MAX_FIX_ROUNDS >= 1)` compile-time invariant plus
  a `Result::Err` return** for the unreachable branch. Catches the panic
  class with the type system; converts the residual unreachable case to a
  clean error if a future refactor changes the loop shape.
- **`llm.rs`: replaced `panic!()` in test match-arms with
  `unreachable!("unexpected variant: {:?}", other)`** for clearer failures
  if the enum gains a new variant.
- **`enterprise.rs`: audit log + cost log now go through
  `secrets::ensure_secret_file`** before the first `OpenOptions::append`,
  so on Unix the file is created with mode 0600 instead of the default
  process-umask.

### Security

- **`swebench_tools::execute_run_command` no longer pipes ReAct-controlled
  strings to `sh -c`.** The previous substring blocklist
  (`rm -rf /`, `shutdown`, `mkfs`, `> /dev/`) was trivially bypassable —
  `rm  -rf  /` (double space), `$(echo rm) -rf /`, ``rm -rf $(echo /)``,
  `/bin/rm -rf /`, and forkbombs all slipped through. The new implementation
  parses argv with `shell-words` and runs `Command::new(argv[0]).args(...)`
  directly, so shell substitution is never interpreted. argv[0] must be in
  `ALLOWED_RUN_COMMAND_HEADS` (pytest, python/python3, pip/pip3, ls/cat/grep,
  git, make, cargo, etc.). Compound-shell tokens (`&&`, `||`, `;`, `|`,
  redirects) are rejected loudly rather than silently misexecuted. Also
  fixes the `python ` substring rewrite that corrupted strings like
  `pythonic_test_file`.
- **`sandbox::validate_path_within` now uses Component-walk + canonicalize.**
  The previous `relative.contains("..")` check rejected legitimate filenames
  like `file..py` (false positive) while accepting pre-planted symlinks
  inside the workspace that pointed at `/etc`. The new implementation walks
  the path components to detect `Component::ParentDir` precisely, then
  canonicalizes both root and the deepest existing ancestor of the joined
  path so symlink-escapes are caught. Drive-letter prefixes (`C:\foo`,
  `D:foo`) are rejected on all platforms.
- **`swebench_tools::resolve_path` delegates to `validate_path_within`.**
  The previous trim+join implementation silently stripped leading `/` and
  rejected `..` only as a substring. Now any unsafe path produces an error
  rather than silent rewriting.
- **`sandbox` env-var stripping switched from substring blocklist to
  allowlist.** The old patterns (`API_KEY`, `SECRET`, `TOKEN`,
  `PRIVATE_KEY`, `PASSWORD`, `CREDENTIAL`) missed `OLLAMA_HOST` (network
  pivot), `DATABASE_URL` / `POSTGRES_URL` / `REDIS_URL` (URLs embed creds),
  `KUBECONFIG` (cluster access), `SSH_AUTH_SOCK` (agent forwarding),
  `AWS_ACCESS_KEY_ID` (caught only via cascade through `KEY`, fragile), and
  several other leak surfaces. Subprocess env is now `env_clear()`'d and
  re-populated from a tight allowlist of universal essentials (PATH, HOME,
  USER, SHELL, LANG, TZ, TMPDIR/TEMP/TMP, TERM), Python venv vars
  (VIRTUAL_ENV, PYTHONUNBUFFERED, PYTHONDONTWRITEBYTECODE), Windows
  essentials (USERNAME, USERPROFILE, SYSTEMROOT, COMSPEC, PATHEXT, etc.),
  and the `LC_*` locale family.
- **`cto.rs` web tools wrap output in `<untrusted source="...">…</untrusted>`
  blocks.** `web_search` (Brave + DuckDuckGo) and `web_fetch` previously
  fed attacker-controllable web content directly into the CTO model
  context. The system prompt now instructs the model to treat
  `<untrusted>` content as data, never instructions. The provenance URL
  is HTML-attribute-escaped to prevent close-tag injection.
- **`cto::web_fetch` SSRF guard.** Validates URL scheme is http/https,
  rejects `localhost` and `*.localhost` hosts, and for literal-IP URLs
  rejects RFC1918 (`10.*`, `172.16/12`, `192.168/16`), link-local
  (`169.254/16`, including the 169.254.169.254 cloud-metadata endpoint),
  loopback, multicast, IPv6 unique-local (`fc00::/7`) / link-local
  (`fe80::/10`), and IPv4-mapped IPv6 loopback / RFC1918 / link-local.
  DNS-rebinding is not defended (would require post-resolve revalidation);
  documented as a known limitation in `SECURITY.md`.
- **`cto::save_history` writes via `secrets::write_secret_file`.** The
  chat history (which can contain sensitive query content) is now written
  atomically with mode 0600 on Unix, instead of the default-umask
  `File::create` it used previously.
- **Regression tests added** for every security fix: the argv-allowlist
  rejects command-substitution and compound metachars, the python rewrite
  doesn't corrupt non-head tokens, `validate_path_within` rejects planted
  symlinks (Unix-only), allows `file..py`, rejects Windows drive-letter
  prefixes, and the env allowlist enumerates the secret patterns that
  must stay stripped.

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

[Unreleased]: https://github.com/mrdushidush/battle-command-forge/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/mrdushidush/battle-command-forge/releases/tag/v0.2.0
[0.1.0]: https://github.com/mrdushidush/battle-command-forge/releases/tag/v0.1.0
