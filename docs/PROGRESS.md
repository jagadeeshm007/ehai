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

### M4 — Env provisioning (2026-11-17)
- `ehai-env`: discover → materialise → validate → ports → install → smoke, each step logged into a `Report`:
  - **Discover:** walks the source checkout (skipping `node_modules`/`.venv`/`target`/etc.), matches built-in patterns (`.env*`, `*.local.*`, `.npmrc`, `local.properties`, `google-services.json`, `.tool-versions`, …) plus `[env]` include/exclude globs, keeps only what git doesn't track in the target (ignored or simply new).
  - **Materialise:** copy (default) or symlink; refuses to overwrite a path the target worktree already tracks.
  - **Validate:** per-directory `.env.example`/`.env.sample` vs sibling `.env*`, missing keys reported by name only, prefixed by app dir in a monorepo.
  - **Ports:** a locked JSON registry at `~/.ehai/ports.json`, keyed by the worktree's canonical path; stable across re-syncs, released on demand. Written to `.env.ehai.local` with a stack-appropriate var name (`RCT_METRO_PORT`, `VITE_PORT`, `PORT`), prefixed by app name when there's more than one app.
  - **Install:** lockfile → package manager detection (pnpm/yarn/npm/bun/go/uv/poetry/pip); a missing tool or lockfile is reported as skipped, never fails the sync. `pod install`/Gradle stay opt-in (`rn_native_install`), per spec.
  - **Smoke:** the configured command per app path, with a timeout; `ehai env doctor` runs just this (plus `check`) with nothing written first.
- CLI: `ehai env sync|check|doctor [path]` (defaults to the current directory; `sync` resolves the source checkout to copy from via the worktree's common `.git` dir).
- MCP tools: `env_sync`, `env_check`.
- Verified against synthetic fixtures for every stack in spec §13 (RN Expo, bare RN, Vite React, Next.js, Go API, FastAPI, pnpm monorepo) reaching "ready", plus property-style coverage: ports never collide across 20 parallel worktrees, materialised env files never show up in `git status`, a missing package manager is reported as skipped rather than failing the whole sync.
- Coverage wasn't independently measured with `cargo-llvm-cov` (not installed in this environment); each module carries its own unit tests and the fixtures suite exercises the full `sync()` path per stack.

### M5 — POC engine (2026-11-17)
- `ehai-stack`: detects stack per-app (RN Expo/bare, React web/Next.js/Vite/CRA, Go, Python); extracts design tokens (Tailwind config, CSS variables, MUI/Chakra/shadcn themes; NativeWind/restyle/tamagui for RN). Writes `.ehai/cache/design-profile.json`, invalidated by file hash. Inventories components, routes/nav stacks, state management, test frameworks, formatters, commit style, PR templates.
- `ehai-poc`: full POC lifecycle — `poc_new` (branch `ehai/poc/<feature>/poc-<n>`, worktree via `ehai-env`, design context), `poc_iterate` (new branch from parent), `poc_list`, `poc_discard` (archive tag + worktree removal), `poc_compare` (side-by-side HTML gallery + diff summary), `poc_open` (start dev server on allocated port), screenshot capture via Chrome CLI (`--headless --screenshot`), degrade gracefully when Chrome is absent.
- Manifest at `.ehai/pocs/<feature>/manifest.json`: id, parent(s), parts, prompt, feedback, status, screenshots, port, created_at, env snapshot hash.
- MCP tools: `poc_new`, `poc_iterate`, `poc_list`, `poc_discard`, `poc_compare`, `poc_open` (6 tools total). Design context returned to the AI for code generation; ehai never generates the React/RN code directly.
- Sandbox: `SandboxMode::Restricted` checks that every worktree lives under `~/.ehai/worktrees`; off-path worktrees are blocked before any file write.
- CLI: `ehai poc new|iterate|list|discard|compare|open`.
- Verified: worktrees created outside the repo; env files never staged; ports unique across concurrent POCs; screenshot degrades cleanly when Chrome is absent.

### M6 — Compose + promote + preferences + role-aware output (2026-11-17)
- `poc_compose`: synthesises a new POC from multiple parents by reading their diffs and rebuilding the requested parts; lineage records `parts: [(parent_n, description)]` per parent POC. Not a cherry-pick.
- `poc_promote`: opens a GitHub PR from the POC branch to the default branch; sets `status = Promoted`; always requires explicit `--yes` confirmation or `needs_input` via MCP. Blocked by sandbox in Restricted mode.
- `ehai-prefs`: flat `[[entries]]` TOML store (`~/.ehai/prefs/user.toml` global, `<repo>/.ehai/prefs/project.toml` per-repo, gitignored by default). Fields: key, value, scope, source (explicit/observed), confidence, first_seen, last_seen. Repo entries override global by key. `to_context_text()` injects a compact block into every POC/planning prompt. `ehai://prefs` MCP resource returns role + project_admin + learned prefs.
- MCP tools added: `poc_compose`, `poc_promote`, `prefs_list`, `prefs_set`, `prefs_delete` (5 new tools).
- Role-aware PM output: `do_save_prd` and `do_status` include `role` and `presentation_hint` fields so Claude can tailor its response format for PM vs dev.
- `ehai-tracker`: `create_pull_request` (REST `POST /repos/.../pulls`); 422 (branch already has open PR) mapped to a clear `Error::Api`.
- CLI: `ehai poc compose|promote`; `ehai prefs list|set|delete`.
- Known gap: per-worktree harness-scoped sandbox settings (§9A, last bullet) — fixed before M8.

### M7 — All harness adapters (2026-11-17)
- 7 new harness adapters: Codex CLI, Gemini CLI, Cursor, OpenCode, VS Code, Windsurf, AGENTS.md (universal).
- Shared `json_mcp.rs` module handles read/backup/patch/write for the 5 JSON-based adapters.
- Marked-block strategy for Codex (TOML `# >>> ehai >>>`) and AGENTS.md (HTML `<!-- >>> ehai >>> -->`).
- VS Code uses `servers` (not `mcpServers`) and requires `type: "stdio"` — the only harness that differs.
- Each adapter has install/idempotency/remove tests and an insta snapshot for `generate()`.
- `ehai_adapters::all()` now returns all 9 adapters; CLI commands require no changes.
- Config paths verified against official docs and recorded in ADR-0026.
- 25 adapter tests: 25 passed, 0 failed.

### Pre-M8 fixes (2026-11-17)

- **Local tracker** (`ehai-tracker::local::LocalTracker`): offline fallback storing items in `~/.ehai/tracker.json` (or `EHAI_LOCAL_TRACKER`). Implements the full `Tracker` trait: create/update/list/get/status/comment/link_pr/assign/unassign. `ensure_schema` is a no-op (schemaless JSON). PR links and comments are appended to the item body. `ehai-id` footer preserved across all mutations. Atomic write via `write_atomic`; in-process `Mutex` guards concurrent MCP calls. 7 unit tests.
- **`AnyTracker` enum** (`ehai-tracker::any`): type-erased dispatcher implementing `Tracker` over `GithubProjects | LocalTracker` via `async fn` delegation, so `sync::sync` and other generic callers work with either backend without `async_trait` or heap boxing.
- **MCP `tracker()` method**: now checks `config.tracker.kind` — `TrackerKind::Local` returns `AnyTracker::Local`, everything else goes through the GitHub client. All 5 PM tool call sites (`do_sync`, schema, `do_triage`, `do_assign`, `do_comment`) work with both backends unchanged.
- **`ehai init` wizard**: detects missing `gh` / no `GITHUB_TOKEN` / no `GH_TOKEN` and asks "Use local storage (all PM flows work offline)" vs "Install gh first". Choosing local writes `tracker.kind = "local"` to config and shows the step in the apply checklist.
- **`TrackerKind::Local`** added to `ehai-core::config`.
- **Per-worktree harness sandbox settings** (spec §9A, last bullet — closes the M6 "known gap"): `ehai-poc::write_worktree_sandbox_settings` writes `.claude/settings.local.json` into every new POC worktree — allows edits/local commands, denies `git push`, resets to a protected branch, and destructive `rm -rf` outside the worktree. Idempotent (skipped if the file already exists, e.g. iterating on the same worktree).
- **`plan_explore` MCP tool** (`ehai-planner::explore::find_gaps`): a lite gap-finder run automatically after `save_prd` — flags a PRD with no flows (or a single happy-path flow with no error handling), an unmeasured metric, an unmitigated risk, empty out-of-scope, or a single user persona; capped at 5 gaps, in priority order. Takes optional `answers` to patch the PRD in place (still PHI-guarded) and re-save it. Not part of the original milestone list — added as a cheap way to improve PRD quality without a second full LLM pass.
- **`ehai-prefs` observation hook**: `EhaiServer::observe_interaction` records lightweight behavioural signals (`last_plan_complexity`, `requirements_completeness`, `poc_created`) into the global prefs store after `save_prd`/`plan_explore`/`poc_new`; fire-and-forget (a prefs write failure never blocks the tool), and self-compacts observed (not explicit) entries once there are more than 50.
- **`ehai migration export|import`**: bundles `config.toml`, `prefs/user.toml` and the local tracker's `tracker.json` into one ZIP for moving to a new machine (harness adapter files are excluded — re-run `adapters regenerate` after importing). `release.rs` adds the embedded minisign public key for M8's signature verification.
- Fixed a dead/no-op condition in `explore::find_gaps`'s single-flow check (`.contains(|c| matches!(c, 'e'))` is true for almost any English text, silently defeating the real "mentions error/fail/fallback" check); added tests for both sides of that branch.

### M8 — Distribution (2026-11-18)
- `ehai-update`: check → stage (download, verify, extract) → swap in → verify → rollback-on-failure.
  - **Check:** GitHub Releases REST API (not GraphQL — releases don't need it). Channel acceptance follows spec §11A: alpha accepts every tag, beta skips `-alpha` tags, stable wants no pre-release suffix at all. Releases come back newest-first from GitHub, so "the latest the channel accepts" needs no semver sorting of its own, just a first-match scan — same trick `install.sh` reuses in shell.
  - **Stage:** downloads the platform's asset, `SHA256SUMS` and `SHA256SUMS.minisig`; verifies the signature (`minisign-verify`, pure Rust, no `minisign` binary needed) against `ehai_core::release::MINISIGN_PUBLIC_KEY`, then the checksum, then extracts the `ehai`/`ehai.exe` binary from the `.tar.gz`/`.zip`. Takes the public key as a parameter rather than reading the constant directly, so it's fully testable against a throwaway test keypair — no production secret needed to test it.
  - **Swap in / rollback:** backs up the running binary to `<exe>.prev`, writes the new one, and calls `self-replace` (handles the Windows "can't overwrite a running exe" case). Deliberately *not* unit-tested directly — `self_replace` always targets `std::env::current_exe()`, so a test calling it would replace the actual test binary. Instead verified via a disposable helper binary (`src/bin/update_test_harness.rs`) spawned as its own subprocess, so `current_exe()` points at a tempdir copy — proves the real swap/backup/rollback mechanics end to end without ever touching `target/debug` or the real `ehai` binary.
  - **Post-update check runs as a fresh subprocess** (`<new-exe> doctor --post-update`), not in-process — the swap only changes the file on disk; the process that performed it is still running the old code until it exits.
  - **Daily notify-only check** (`update.check`): a `~/.ehai/update-check.json` timestamp gates it to once a day; runs after a command's own output (never delays the thing the user asked for), with a short 3s timeout, and is silently skipped on any failure. Off for `mcp` (must keep stdout clean), `update` itself, `doctor` and `uninstall`.
- CLI: `ehai update [--channel alpha|beta|stable] [--rollback]`.
- `ehai migration export|import` → `ehai-update` relationship: migration is unrelated (device move, not binary update) but shares the ZIP/verify plumbing conceptually; both live in `ehai-cli`.
- `install/install.sh` (macOS/Linux) and `install/install.ps1` (Windows): `gh auth token` → `GITHUB_TOKEN` fallback, picks the release for `$EHAI_CHANNEL` by scanning releases newest-first (same rule as the Rust side), verifies with the system `minisign` binary (required — the installer refuses to run without it) and a checksum, then runs `self install` and `init`. `install.ps1` also sets the user `PATH` registry entry itself, since `self install` on Windows explicitly defers that to "the installer" (confirmed by reading `selfcmd.rs`'s own Windows-branch message).
- `justfile`: `just release <version>` bumps the workspace version, regenerates `CHANGELOG.md` via `git-cliff`, commits and tags (doesn't push).
- CI: new `shellcheck` job lints `install.sh` (mirrored in `ci-skip.yml`).
- Config migration mechanism (`ehai_core::migrate`) refactored so the forward-migration loop (`migrate_with`) is parameterised over the steps array and target version, letting a test simulate a v2 schema (key rename + new default field) without touching the real `STEPS`/`CURRENT_SCHEMA_VERSION`, which stay at v1 until there's an actual v2.
- Found and fixed in review: the background daily-check timestamp file, and the local tracker's `tracker.json`, weren't in `selfcmd::OWNED` — `ehai uninstall` was leaving them behind as unrecognised "leftovers" and therefore refusing to remove `~/.ehai` itself. Added both.
- **Not done in this milestone's commit:** the first real push + tag. Nothing in M8's commit touches the git remote — see M8.1 below for that.

### M8.1 — First push (2026-11-18)
- **D1 resolved:** the repo stays **public** (user's explicit choice). ADR-0001 amended; CLAUDE.md §11/§14 and `docs/milestones.md` updated to stop describing a private-repo requirement that never had a code dependency — `install.sh`/`install.ps1`/`ehai-update` already treated auth as optional, not required.
- **D2 resolved:** the user generated the real minisign keypair (`~/.ehai/minisign.pub`/`.key`, matching the placeholder embedded in `ehai_core::release::MINISIGN_PUBLIC_KEY` — same key, so no code change needed) and added the secret key as `MINISIGN_KEY` in `jagadeeshm007/ehai` → Settings → Secrets and variables → Actions.
- First push to `main`. CI caught real issues plain `cargo build`/`test`/`clippy` locally never would — see ADR-0029 for the full list. Fixed across several follow-up pushes:
  - `zip` 2.6.1 had two real RUSTSEC vulnerabilities (bumped to 9.x, trimmed to `features = ["deflate"]` to avoid new transitive licenses); `chrono` is unmaintained with no fix available (accepted via `deny.toml`'s advisory ignore list).
  - Two test-only platform assumptions broke on the 3-OS matrix: `ehai-env`'s install-skip test assumed `yarn` was absent (true here, false on GitHub's runners — fixed with an `EHAI_ENV_PATH` override instead of mutating real `PATH`); `ehai-update`'s fixture hardcoded one target (fixed with a multi-target signed fixture).
  - `cargo insta test`'s CI invocation had `--locked` in the wrong place (doctests reject it outright).
  - The repo had no `.gitattributes`, so Windows checkouts got CRLF for the `templates/**/*.md` skill files, which are hashed into generated adapter content at runtime (`claude_code.rs`) — different hash, different literal text, snapshot mismatch. Added `.gitattributes` forcing LF everywhere.
  - `ehai init` now runs the equivalent of `self install` as its first step, unconditionally — PATH setup and a stable binary location for harness adapters no longer need a separate command.
- CI result after the `.gitattributes`/auto-install fixes tracked in the next push: **all jobs green** (run 44162872235).

## Post-M8 additions (pre-M9)
- **`plan_explore` MCP tool** (ADR-0031): deterministic gap-finder auto-run after every `save_prd`; skill template drives a max-3-round refinement loop with top-2 questions per round.
- **Claude Desktop skills ZIP** (ADR-0030): `install()` generates `{brand}-skills.zip` with the 5 PM skill `.md` files; `remove()` deletes it.
- **`ehai migration export|import`**: device-move ZIP (config + prefs + local tracker); harness adapters excluded (re-run `adapters regenerate` after import).
- **`observe_interaction` hook**: lightweight behavioural signals written fire-and-forget after `save_prd` and `poc_new`; self-compacts above 50 entries.
- **Template updates**: `plan/SKILL.md` and `shared/commands/plan.md` include the `plan_explore` refinement step (4b); 7 harness adapter snapshots updated accordingly.
- **`ehai poc run` / `ehai poc shell`** (2026-11-19): run a command (default: the dev server) or open a shell inside a POC worktree from anywhere in the repo, with its `.env.ehai.local` ports loaded and the exit code passed through. `ehai_env::read_env_overlay` reads the overlay back.
- **★ Insights footer** (2026-11-19): every skill (via `{{role_block}}`), AGENTS.md (`{{insights_block}}`) and the MCP server instructions tell the agent to end replies with a 2–5 line footer (summary, status, next command, ⚠ warnings). `ui::insights` renders the same footer for `poc new|open|list`.

## Next — M9: Hardening and release

## Open questions
- **D1 (repo visibility) — resolved 2026-11-18:** stays public.
- **D2 (signing key) — resolved 2026-11-18:** real keypair generated, `MINISIGN_KEY` secret added, public half already matched the embedded constant.
- **D3 (OAuth app):** still deferred per ADR-0017; `gh`/`GITHUB_TOKEN` cover everything `ehai-update` and `install.sh` need.
- **Claude desktop skills ZIP — resolved 2026-11-18 (ADR-0030):** `ehai connect claude-desktop` now also generates `~/.ehai/adapters/claude-desktop/{brand}-skills.zip` with the 5 PM skill `.md` files; upload via Claude Desktop → Settings → Skills to surface `/ehai:plan` etc. as discoverable commands. ADR-0022's "prompts are sufficient" held for the half-finished attempt; it no longer applies to the complete implementation.
- The interactive wizard path (cliclack on a real TTY) is exercised manually. Automated tests cover the same logic through the plain/scripted prompter.
- `install.ps1` is untested in this environment (no PowerShell available) — syntax reviewed by hand only; needs a real Windows run before `v0.1.0-alpha.1`.
