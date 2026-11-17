# Progress

## Done

### M0 — Skeleton (2026-11-17)
- Cargo workspace at `0.1.0-alpha.1` (`version.workspace = true`), MSRV 1.89, stable toolchain pinned.
- `ehai-core`:
  - paths, with `EHAI_HOME` override
  - brand name, with validation and derived MCP name / command prefix
  - typed config schema v1 with JSON schema
  - layered loading: defaults < global < repo `.ehai.toml` < `--config` < `EHAI_*` env < CLI
  - forward-only migration framework, with backup, atomic write and file lock
- `ehai-cli`:
  - `ehai --version` (version, sha, build date, target)
  - `ehai doctor [--post-update]` checks binary, config, git ≥ 2.20, home writable and gh (optional)
  - `ehai config show|path|schema`
  - JSON logs in `~/.ehai/logs` (daily, 14 kept)
- UI kit: a `Prompter` trait with cliclack (TTY) and plain/scripted (CI and tests) implementations, plus ✔/✖ checklist steps with fix hints.
- CI: path-filtered `ci.yml` plus a `ci-skip.yml` mirror. Jobs: fmt, clippy, nextest on 3 OSes, cargo-deny, coverage ≥ 80%, insta check.
- Release: tag-only `release.yml`.
  - Checks that the tag equals the workspace version.
  - Builds musl/macOS/Windows.
  - Produces archives, SHA256SUMS, minisign, git-cliff notes, and marks pre-release tags as pre-releases.
- Conventional-commit hook (`.githooks/commit-msg`), `cliff.toml`, `deny.toml`.

## Next — M1: MCP + Claude Code + init wizard
- `ehai-mcp`: rmcp stdio server, `ping` tool, `ehai://prefs` resource, panic-safe tool boundary.
- Claude Code plugin adapter (local marketplace). **Verify the current plugin and MCP formats in the docs first.**
- Native-question pattern in templates (`needs_input` fallback).
- The full `ehai init` wizard, built on `ui::Prompter` (auto-detect, summary card, ≤ 5 questions, live checklist).
- Repo onboarding (`ehai repo init`), role selection.
- Esc-to-go-back in wizards. cliclack returns an error on Esc, so the wizard needs a step stack.

## Open questions
- `MINISIGN_KEY` secret: generate with `minisign -G -W` and add it to `jagadeeshm943/ehai` before the first tag.
- Should the Claude desktop integration use stdio or streamable HTTP? Decide in M1/M3 after checking the current docs.
