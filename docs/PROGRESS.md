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

### Post-M1 fixes (2026-11-17)
- `ehai` was not on PATH, and the plugin pointed at `target/debug`.
  - `ehai self install` now installs to `~/.ehai/bin` with a marked PATH block.
  - Adapters prefer the installed binary.
- `ehai connect <id>` / `ehai disconnect <id>|--all` connect and disconnect harnesses.
- `ehai uninstall` removes everything in one command and is tested to leave no traces.

### M2 — Tracker (2026-11-17)
- `ehai-tracker`:
  - `Tracker` trait, status/priority/size/type enums, and an `ehai-id` footer.
  - `MemoryTracker` (the reference implementation).
  - The GitHub Projects v2 adapter: issues in a repo or draft items, a non-destructive `ensure_schema`, pagination, and retries with backoff.
  - The idempotent sync engine (`sync::plan` / `sync::sync`).
  - GitHub auth via `gh` or env, with `project`-scope detection.
- Shared contract suite: runs against the memory tracker and against GitHub on a stateful fake server (`fake` feature). It includes a test that a schema update keeps existing card statuses.
- Opt-in live test (`EHAI_LIVE_GH=1`): the full run passes against real GitHub as `jagadeeshm007` (ADR-0019).
- CLI:
  - `ehai tracker set [--owner --project|--create --repo|--no-repo]`, `tracker status` and `tracker schema`.
  - `init` gained a "Where should work be tracked?" step (defaults to Skip, plus `--tracker-project/--tracker-create/--no-tracker`).
  - `doctor` checks the token, the scope and project reachability.
- The release repo moved to `jagadeeshm007/ehai`.

### M3 — PM flow (2026-11-17)
- `ehai-guard`:
  - Detects SSN, MRN, DOB, member id, email, phone, address, name + medical term, plus custom patterns.
  - Confidence levels; off/warn/strict modes.
  - A value-free JSONL audit log.
  - Property-tested: no false negatives on generated PHI.
- `ehai-planner`:
  - PRD and breakdown models with validation (measurable metrics, Mermaid flows, dependency cycles, acceptance criteria).
  - Markdown, Mermaid and board preview.
  - Conversion to tracker items.
  - Status report (Markdown + Mermaid + self-contained HTML).
  - Triage duplicate detection.
- MCP server:
  - 9 tools (`save_prd`, `get_plan`, `list_plans`, `save_breakdown`, `sync_plan`, `status_report`, `triage`, `guard_check`, `ping`).
  - 7 prompts, each mirroring a skill.
  - `needs_input` questions; every write is guarded and audited.
- Claude Code skills `/ehai:plan`, `breakdown`, `sync`, `status` and `triage` (role-aware, native questions, no real patient data).
- Claude desktop adapter: an `mcpServers` entry in `claude_desktop_config.json`; the flows arrive as MCP prompts.
- Verified live: the plugin passes `claude plugin validate`, the MCP server shows ✔ Connected, and the real binary serves 9 tools and 7 prompts.

### M3.5 — Roles and team (2026-11-17)
- Role resolution tied to real GitHub project admin status (`docs/plans/roles-and-team.md`, approved):
  - Admin check: `ProjectV2.viewerCanClose`, run when `tracker set`/`init` connects a project and stored as `access.project_admin` + `access.checked`.
  - `Config::effective_role()` reads `access.project_admin` at every call site; a stored `role` or `EHAI_ROLE` override is harmless to persist — it only takes effect for an admin. Non-admins and "no project connected yet" are always `dev`, no question asked.
  - `ehai role` shows the effective role and why; `ehai role set <role>` is refused with a clear message for non-admins.
- `ehai-tracker::team`: team = repo assignable users ∪ org members (merged by login), one combined GraphQL query, cached at `~/.ehai/cache/team/<owner>-<number>.json`, refreshed once a day or with `ehai team --refresh`; falls back to the stale cache if a refresh fails.
- Assignment, by design, is **not permission-gated** — every action stays available to every role; role only shapes tone and defaults. `ehai-tracker::Tracker::assign`/`unassign`:
  - Issues: `addAssigneesToAssignable` / `removeAssigneesToAssignable`.
  - Draft issues: `updateProjectV2DraftIssue` (replace-only — no additive mutation exists for drafts), so the impl reads current assignees first and replaces the whole list.
- `sync`: `Desired.assignees` are set once, on create only — a re-sync never undoes a manual reassignment on the board (same rule as status).
- MCP tools: `team`, `assign_task` (add/replace/remove modes), `comment_task` (optional `@login` mentions, still PHI-guarded). Both resolve a fuzzy `item` (`#18`, URL, ehai id or title substring) and fuzzy assignee names, returning `needs_input` on ambiguity.
- `ehai-planner`: `Story`/`Task` gained an optional `assignees` field (tasks inherit the story's when empty, like `platforms`); `status_report` gained a by-person table and an "overloaded" (>5 open) flag.
- Claude Code skill `/ehai:assign`; `doctor` shows the role, why, and the team cache age.
- Verified live against `jagadeeshm007/aisdlc-demo` (real admin check, team fetch, role persistence) and against a disposable throwaway GitHub project (`EHAI_LIVE_GH=1`, self-deleting) covering admin check, team fetch and assign/unassign round-trip.

## Next — M4: Env provisioning
- `ehai-env`: discover gitignored config (`.env*`, local configs, native files), copy or symlink, check against `.env.example`, isolate ports (`.env.ehai.local`, `~/.ehai/ports.json`), install dependencies per stack, run a health check, and never stage env files.
- `ehai env sync|check|doctor`, plus the same as MCP tools.
- Fixture repos: RN Expo, Vite React, Next.js, Go, FastAPI and a pnpm monorepo.

## Open questions
- OAuth device flow needs an ehai OAuth App client id (ADR-0017). Who registers it, and under which org?
- `MINISIGN_KEY` secret: generate with `minisign -G -W` and add it to `jagadeeshm007/ehai` before the first tag.
- Claude desktop: are ZIP skill uploads wanted in addition to the MCP prompts (ADR-0022)?
- The interactive wizard path (cliclack on a real TTY) is exercised manually. Automated tests cover the same logic through the plain/scripted prompter.
