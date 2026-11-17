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

### M1 — MCP + Claude Code + init wizard (2026-11-17)
- `ehai-mcp`: rmcp 3.10 stdio server.
  - `ping` tool and `ehai://prefs` resource.
  - A per-handler panic guard, so a bad request never crashes the server.
  - Clean exit when the client disconnects.
- `ehai-adapters`: a `Harness` trait plus the Claude Code adapter.
  - Generates a local marketplace whose plugin bundles the MCP server and skills (`/ehai:help`, `/ehai:prefs`).
  - Install is idempotent, and the plugin version is content-hashed so updates are picked up.
  - Handles brand renames, and `remove` reverses everything.
  - Verified live with Claude Code 2.1.179: `claude plugin validate` passes and the MCP server shows ✔ Connected.
- Templates: shared "ask the user" (native question UI + `needs_input`) and role blocks, rendered with the brand.
- `ehai init` wizard:
  - Auto-detects OS, git, gh + auth, harnesses, Claude desktop, Chrome and the repo.
  - Shows a summary card, then asks ≤ 5 questions, each skippable by a flag.
  - Esc goes back, and a cancel writes nothing.
  - Applies with a live checklist and retry, then ends with next commands per role.
- `ehai-stack`: repo scan.
  - Apps and stacks: Expo/RN/Next/Vite/CRA/React/Go/FastAPI/Django/Flask.
  - Package managers, design-system libraries, env file names, and conventions (tests, lint/format, commit style, PR template).
- `ehai repo init` writes `.ehai/profile.toml`.
- `ehai adapters list|install|remove|regenerate`. `doctor` now verifies the enabled adapters.
- Core additions:
  - `config::edit_file`, which preserves unknown keys and validates before writing
  - a git wrapper
  - a process runner with timeouts
  - reserved brand names

## Next — M2: Tracker
- `Tracker` trait plus the GitHub Projects v2 adapter (GraphQL, reqwest + rustls), with contract tests (wiremock) and an opt-in live test (`EHAI_LIVE_GH=1`).
- GitHub auth: `gh auth token` → `GITHUB_TOKEN`/`GH_TOKEN` → device flow, with the token in the keychain (`keyring`) or a 0600 file.
- `ehai tracker set`, `ensure_schema`, and the idempotent sync engine (`ehai-id` footer).
- Add the tracker + project question to `init`, with the project list fetched live and a "Create new" option (ADR-0013).

## Open questions
- `MINISIGN_KEY` secret: generate with `minisign -G -W` and add it to `jagadeeshm943/ehai` before the first tag.
- Claude desktop integration (M3): stdio via `claude_desktop_config.json`, or streamable HTTP. Verify the current docs first.
- The interactive wizard path (cliclack on a real TTY) is exercised manually. Automated tests cover the same logic through the plain/scripted prompter.
