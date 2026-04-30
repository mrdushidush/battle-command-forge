# Security policy

Thank you for helping keep BattleCommand Forge and its users safe.

## Supported versions

Only the latest released version on `main` is actively supported. We
do not backport fixes to prior minor versions.

| Version | Supported |
|---------|-----------|
| `v0.2.x` | ✅        |
| `< 0.2`  | ❌        |

## Reporting a vulnerability

**Please do not open a public GitHub issue for security reports.**
Open issues are visible to everyone and give attackers a head start.

Instead, file a private report through GitHub's security advisory
system:

1. Go to <https://github.com/mrdushidush/battle-command-forge/security/advisories/new>
2. Fill in the form — include a description, affected versions, a
   reproducer if you have one, and your estimate of impact.
3. Submit. Only the repo maintainers will see it.

If for any reason the security advisory flow is unavailable, email
the repo owner at the address listed on the GitHub profile page.

## What to expect

- **Acknowledgement:** within 7 days (often same-day — this is a
  solo-maintainer project, so responsiveness depends on other
  demands).
- **Triage:** confirmation that we can reproduce, plus initial
  severity assessment, within 14 days.
- **Fix:** timeline depends on severity. Critical issues are
  prioritised above all other work; medium issues land in the next
  scheduled release; low issues may be deferred and documented.
- **Disclosure:** coordinated. We'll work with you on a public
  disclosure date. Credit in the release notes if you'd like it.

## Scope

In scope:

- Vulnerabilities in BCF's own code (anything under `src/`).
- Sandbox escapes from the `output/<mission>/` workspace —
  particularly any path-traversal that lets a generated project read
  or write outside its own directory.
- Bypasses of the SWE-bench `run_command` argv allowlist that allow
  arbitrary host-side execution.
- Bypasses of the env-var allowlist that leak provider API keys
  (`ANTHROPIC_API_KEY`, `XAI_API_KEY`, `BRAVE_API_KEY`) or other
  secrets to spawned subprocesses.
- Prompt-injection attacks via `web_fetch` / `web_search` tool output
  that escape the `<untrusted>` provenance boundary in the CTO chat
  agent.
- Incorrect handling of secrets (chat history, audit log) — e.g.
  files written world-readable, leaked through logs, or persisted
  unintentionally.

Out of scope:

- Vulnerabilities in upstream dependencies. Report those to the
  upstream project; we'll bump the dep version once a fix is
  available.
- Vulnerabilities in Ollama, the Anthropic API, the xAI API, the
  Brave Search API, or DuckDuckGo. Those are separate surfaces we
  consume.
- Denial-of-service attacks against the local process. BCF is a
  single-user tool; if someone has the ability to spam your local
  Ollama or burn your API quota, they already have code execution on
  your machine.
- Issues that require the attacker to already control the user's
  machine (e.g. tampering with `~/.battlecommand/` files).

## Threat model we target

BCF is designed for a single-user, local-deployment threat model:

1. The user trusts their own machine.
2. The user trusts themselves and any prompts / personas they feed
   into BCF.
3. **Generated project code is untrusted.** The 9-stage pipeline
   produces code from LLM output; that code may contain bugs,
   incorrect logic, or (rarely) hostile patterns reflected back from
   prompt-injected web content. The verifier runs it inside a
   per-project venv with subprocess timeouts and stripped env vars,
   but the user is expected to read the output before running it
   anywhere production-adjacent.
4. **External tool input is untrusted.** Web pages fetched via
   `web_fetch`, search results from Brave / DuckDuckGo, and any text
   the agent retrieves over the network is wrapped in
   `<untrusted source="...">` blocks; the CTO model is instructed
   to treat the content as data, not instructions.

The combination of (a) the argv-allowlist on `run_command`, (b) the
canonicalize-checked path validation on file tools, (c) the env-var
allowlist on subprocess spawn, and (d) the `<untrusted>` provenance
on web tools is the primary defense surface. If you can bypass any
of them, that is a security-relevant bug we want to hear about.

## Non-goals

- **Multi-tenant safety.** BCF is not designed for shared hosting.
  Each user runs their own instance.
- **Adversarial model providers.** We trust Ollama, Anthropic, and
  xAI to return the model output we asked for. If a malicious Ollama
  server returns crafted output, that's equivalent to trusting the
  brain — outside this project's scope.
- **Network interception.** Transport security is whatever
  `reqwest` / `rustls` give us. We don't pin certificates for the
  external APIs we call.

## History

This is the first version of the security policy; no reported
vulnerabilities yet. If that changes, resolved issues will be listed
here with CVE references if applicable.
