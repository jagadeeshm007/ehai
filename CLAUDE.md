# CLAUDE.md — Build "ehai": the AI-SDLC Ops Engine

You are a senior Rust systems engineer and product-minded tool builder. You are building **ehai**, an AI-SDLC operations tool for a **health-tech organization**. Read this whole file before writing code. Follow the milestones in order. Do not skip tests.

> Project state: see `docs/PROGRESS.md` (what's done / next) and `docs/DECISIONS.md` (ADRs). Release repo: `jagadeeshm007/ehai`. Min OS: macOS 12, Windows 10 1809, Linux 5.13 (ADR-0007).

---

## 0. Working rules for you (Claude Code)

- **Ask, don't guess.** Whenever you need information from the user, use the `AskUserQuestion` tool with 2–4 concrete options (put your recommendation first, marked "(Recommended)"). Ask only things that change what you build.
- Work **milestone by milestone** (section 12). At the end of each milestone: all tests green, `cargo clippy --all-targets -- -D warnings` clean, `cargo fmt --check` clean, docs updated, one commit with a clear message.
- Keep a living `docs/PROGRESS.md` (what's done, what's next, open questions) and `docs/DECISIONS.md` (ADR-style, one entry per non-obvious decision).
- Harness config paths and formats (Claude Code, Codex, Gemini CLI, Cursor, etc.) change often. **Verify against current official docs before implementing each adapter**, and record the source + date in `DECISIONS.md`.
- Never commit secrets, `.env` files, or real patient data — not even in tests or fixtures.

---

## 1. Product summary

ehai helps a **non-technical PM** plan product requirements and tasks, pushes them into a tracker (GitHub Projects by default), tracks progress and issues, and helps developers carry tasks through the dev lifecycle — from **any AI coding harness**. It also generates **UI/UX proof-of-concepts (POCs)** that match the existing app's look and feel, in full isolation from the main codebase, with multiple iterations and composition (`poc1 + poc2 → poc4`).

**Users**
- **PM** — mainly uses the **Claude desktop app** (Claude docs, visuals, plain language) through ehai's MCP server + skills.
- **Developers** — use their coding harness (Claude Code, Codex, Cursor, Gemini CLI, OpenCode, Copilot/VS Code, Windsurf, …) through the same MCP server + slash commands.

**Client tech stack (ehai must understand all of these)**
- React Native (Expo and bare), React JS (Vite / Next.js / CRA), Go backends, Python backends (FastAPI / Django / Flask).
- Monorepos and polyrepos.

---

## 2. Non-negotiable principles

1. **One engine, thin adapters.** All logic lives in the Rust engine. Harness integrations are generated prompt/config files that call the engine's MCP tools.
2. **Global, system-wide.** Installed once per machine at user level; works in every repo with no per-repo setup. An optional per-repo `.ehai.toml` can override settings.
3. **One-line install, minimal dependencies.** A single static binary. No Node, Python, or Docker required to run the engine. Use `gh` only if present (to reuse auth), never require it.
4. **Configurable name.** Default name is `ehai`. The display name, binary alias, MCP server name, and slash-command prefix come from `brand.name` in config (default `"ehai"`). Changing it regenerates all adapters. The installer accepts `EHAI_NAME=<name>` to install under a different command name (symlink/alias). The internal crate/config dir stays `ehai`.
5. **Never break, always run.** Updates are atomic and roll back automatically on failure. Config is versioned with migrations. MCP tool schemas evolve backward-compatibly.
6. **Main is sacred.** POCs never modify `main`, the default branch, or the user's current working tree, and are never merged/pushed unless the user explicitly asks.
7. **Ask through native question UI.** Every user-facing question goes through the harness's multiple-choice UI (see section 7).
8. **Learn and adapt.** ehai observes user choices and keeps an editable preferences profile that improves future output (section 8).
9. **Configurable compliance.** Health-tech guardrails, configurable from off to strict (section 9).
10. **Do it for the user.** Setup and day-to-day flows auto-detect and auto-configure as much as possible. Ask only for what cannot be detected, and confirm before risky actions. Default answers are always pre-filled.
11. **Beautiful, smooth terminal UX.** Every interactive terminal flow (install, init, tracker switch, doctor, update) must feel polished: a guided wizard, arrow-key selection, spinners and progress, clear success and error states (section 5.0).
12. **Role-aware, any surface.** A PM may use the Claude desktop app **or the Claude Code CLI**; devs may use any harness. ehai adapts its output to the user's role (section 8.2).
13. **Safe by default.** Agent-driven work (POCs, installs, dev servers) runs with sandboxing and guardrails, configurable (section 9A).

---

## 3. Tech choices

- **Language:** Rust (stable, edition 2021+). Cargo workspace.
- **CLI:** `clap` (derive). **Async:** `tokio`. **Errors:** `thiserror` in libraries, `anyhow` only in the binary. **Logging:** `tracing` + `tracing-subscriber` (JSON file logs in `~/.ehai/logs`, rotated).
- **MCP:** the official Rust MCP SDK (`rmcp`; check the latest version). Transport: stdio (spawned by harnesses). Optional streamable-HTTP mode for Claude desktop if needed.
- **Config:** TOML via `serde` + `toml`; JSON schema generated with `schemars`.
- **GitHub:** `reqwest` (rustls, no OpenSSL), GraphQL for Projects v2 (`graphql_client` or hand-written typed queries).
- **Git:** shell out to the system `git` (worktrees, branches, diffs) through a small, well-tested wrapper. Do not use libgit2 for worktrees.
- **Self-update:** `self-replace` (or equivalent) + sha256 checksum + signature (minisign).
- **Screenshots (POC preview):** drive an existing Chrome/Chromium via CDP (`chromiumoxide`) if available; degrade gracefully if not.
- **Testing:** `insta` (snapshots), `wiremock` (HTTP mocks), `proptest` (config migrations, parsers), `assert_cmd` + `assert_fs` + `tempfile` (CLI + git integration), `cargo-nextest`, `cargo-deny`, `cargo-llvm-cov`.

---

## 4. Workspace layout

```
ehai/
├─ crates/
│  ├─ ehai-core/        # config, paths, versioned schema + migrations, brand/name, errors, event bus
│  ├─ ehai-tracker/     # Tracker trait + adapters: github_projects (default), linear, jira (stubs ok)
│  ├─ ehai-planner/     # PRD model, breakdown (epic→story→task), acceptance criteria, sync diffing
│  ├─ ehai-poc/         # POC lifecycle: worktrees, manifest, lineage, compose, compare, promote
│  ├─ ehai-env/         # worktree environment provisioning (section 6) — critical
│  ├─ ehai-stack/       # stack detection (RN/React/Go/Python), design-token extraction, dev-server runners
│  ├─ ehai-prefs/       # preference learning store (section 8)
│  ├─ ehai-guard/       # compliance/PHI guard (section 9)
│  ├─ ehai-adapters/    # harness installers/generators (section 10), idempotent + reversible
│  ├─ ehai-update/      # self-update, rollback, health check (section 11)
│  ├─ ehai-mcp/         # MCP server: tools, resources, prompts, elicitation
│  └─ ehai-cli/         # binary `ehai`: CLI commands + `ehai mcp` + `ehai doctor`
├─ templates/           # slash-command / skill / AGENTS.md templates (embedded via include_str!)
├─ install/             # install.sh, install.ps1
├─ docs/
└─ tests/e2e/
```

---

## 5. Features

### 5.0 Interactive terminal experience (important)
- **Libraries:** `cliclack` (clack-style guided prompts: intro/outro, select, multiselect, confirm, spinners, notes) for wizards, `indicatif` for progress bars, `console` for color/styling, and `ratatui` for full-screen views (`ehai status --tui`, `ehai poc list --tui`, `ehai prefs --tui`). If you find a better crate, propose it via `AskUserQuestion` and log the decision in `DECISIONS.md`.
- **`ehai init` wizard flow:**
  1. Welcome banner with the configured brand name and version.
  2. **Auto-detect everything first**, with a spinner per check: OS, git, `gh` + auth, installed harnesses, Claude desktop, Chrome, current repo + remote + stack, existing GitHub Projects.
  3. Show a **summary card** of what was detected and the defaults ehai will apply.
  4. Ask only for the gaps, in order: role (PM / Dev / Both), which harnesses to wire up (pre-checked), tracker + project (list fetched live, "Create new" option), compliance mode, sandbox mode.
  5. Apply the changes with a live checklist (✔ / ✖ per step); failures show a fix suggestion and a retry option.
  6. Finish with an outro card: "You're ready". Show the three most useful commands for *this role*.
- **Repo onboarding:** the first time ehai runs in a new repo (via any command), it runs a short detection pass (stack, apps, env files, design system, conventions), shows a one-screen summary, asks to confirm, and writes the repo profile (section 8). The user can rerun it anytime with `ehai repo init`.
- **Rules:**
  - Every prompt has a sensible default (Enter accepts) and Esc goes back.
  - `Ctrl-C` exits cleanly with no half-applied state; steps are transactional or resumable.
  - Non-interactive mode for CI and scripting: `--yes`, `--config <file>`, and flags for every question. `NO_COLOR` and non-TTY output are respected and degrade to plain text.
  - Windows Terminal, macOS Terminal/iTerm, and common Linux terminals must all render correctly.
- **Tests:** test wizard logic separately from rendering, by putting prompts behind a `Prompter` trait with a scripted fake. Snapshot the plain-text output.

### 5.1 Setup and tracker
- `ehai init`: the interactive wizard above. It detects installed harnesses, registers MCP + commands, authenticates GitHub, and picks a tracker.
- **GitHub auth order:** `gh auth token` → `GITHUB_TOKEN`/`GH_TOKEN` → OAuth device flow. Store the token in the OS keychain (`keyring` crate), with a file fallback at mode 0600.
- **Tracker selection:** GitHub Projects (default). Pick the org/user and an existing project, or create one. `ehai tracker set <type>` switches trackers; `ehai tracker migrate --from --to` comes later.
- **`Tracker` trait (async):** `ensure_schema()` (creates Status/Priority/Size/Iteration/Type fields and options), `create_item`, `update_item`, `set_status`, `list_items(filter)`, `comment`, `link_pr`, `get_item`. Every adapter passes the same shared contract test suite.
- **Config:** `~/.ehai/config.toml` (global), optional `.ehai.toml` per repo. Merge order: defaults < global < repo < env vars < CLI flags.

### 5.2 PM planning flow (visual first)
- **`/ehai:plan`** — interview the PM in plain language using multiple-choice questions, then produce a PRD: problem, users, goals, scope / out-of-scope, user flows (Mermaid), simple wireframe descriptions, success metrics, risks. In Claude desktop, write it as a Claude doc. Also store it as `docs/prd/<slug>.md` in the repo when one is available.
- **`/ehai:breakdown`** — PRD → epics → stories → tasks, each with acceptance criteria (Given/When/Then), size, priority, dependencies, and platform tags (`rn`, `web`, `go`, `py`). Show a visual board preview (table + Mermaid dependency graph) and ask for approval before syncing.
- **`/ehai:sync`** — push to the tracker idempotently. Store a stable `ehai-id` in an issue body footer and diff local vs remote, so re-running never duplicates.
- **`/ehai:status`** — progress report: % done per epic, blockers, stale items, open PRs, risk flags. Output Markdown + Mermaid, and optionally a self-contained HTML report at `~/.ehai/reports/`.
- **`/ehai:triage`** — turn a bug or issue description into a classified, prioritized tracker item, with duplicate detection against existing items.

### 5.3 Developer flow
- **`/ehai:next`** — suggest the next task (priority, dependencies, assignee).
- **`/ehai:start <#id>`** — create the branch (`feat/<id>-<slug>`), load the task context and acceptance criteria into the session, and set the status to In Progress.
- **`/ehai:pr`** — draft a PR description that links the issue, maps changes to acceptance criteria, and adds a checklist. Moves the card to In Review.
- **`/ehai:done`** — verify acceptance criteria and move the card.

### 5.4 POC engine (UI/UX)
Goal: a meaningful, polished POC that matches the existing app's theme, colors, typography, spacing, components, and navigation patterns, without disturbing the codebase.

**Pipeline**
1. **Understand** — read the requirement (PRD/task/free text). Ask clarifying multiple-choice questions only if needed.
2. **Analyze the codebase** (`ehai-stack`):
   - Detect the stack and the target app (RN app vs web app in a monorepo).
   - **Extract design tokens:**
     - Web: Tailwind config, CSS variables, MUI/Chakra/Mantine themes, shadcn `components.json`, styled-components themes.
     - React Native: theme files, `StyleSheet` constants, NativeWind config, react-native-paper / tamagui / restyle themes, font assets.
   - Inventory reusable components and layout/navigation patterns (React Router / Next routes, React Navigation / Expo Router stacks and tabs).
   - Write the result to `.ehai/cache/design-profile.json`, invalidated by file hashes.
3. **Product placement** — decide where the feature lives (which nav entry, screen, or modal), what it reuses, and what is new. Present this as a short visual summary before building.
4. **Build** — in an isolated worktree (section 6), using only existing components and tokens. Use mock data only (respect compliance mode). Mount the POC behind a preview-only route/screen or feature flag so existing flows are untouched.
5. **Preview** — start the dev server on an allocated port and capture screenshots:
   - Web: headless Chrome.
   - RN: Expo web if supported, otherwise simulator/emulator screenshot if available, otherwise Storybook, otherwise skip with a clear message.
6. **Register** — save the POC in the manifest with screenshots, prompt, feedback, and lineage.

**Commands**
- `/ehai:poc new "<idea>"` → `poc1`
- `/ehai:poc iterate <pocN> "<feedback>"` → next number, lineage = parent
- `/ehai:poc compose "<take X from poc1, Y from poc2>"` → new POC from the base branch. The AI reads the referenced POCs' diffs and resynthesizes the parts together (not a blind cherry-pick). Lineage records parts per parent.
- `/ehai:poc list` / `compare [pocA pocB …]` — side-by-side screenshot gallery (HTML) plus diff summary
- `/ehai:poc open <pocN>` — run its preview
- `/ehai:poc promote <pocN>` — **only on explicit command:** confirm via the question UI, then open a PR to the default branch
- `/ehai:poc discard <pocN>` — archive as tag `ehai-archive/<feature>/<pocN>` and remove the worktree. Nothing is hard-deleted until one POC is confirmed/promoted; after promotion, ask whether to archive the rest.

**Storage**
- Branches: `ehai/poc/<feature-slug>/poc-<n>`
- Worktrees: `~/.ehai/worktrees/<repo-id>/<feature>/poc-<n>` (outside the repo, so the repo stays clean)
- Manifest: `.ehai/pocs/<feature>/manifest.json` (gitignored by default; config option to commit it). Fields: id, parent(s), parts, prompt, feedback, status (`draft|liked|rejected|promoted|archived`), screenshots, port, created_at, env snapshot hash.

---

## 6. Worktree environment provisioning (`ehai-env`) — critical

**Problem to solve:** a new git worktree lacks gitignored config (`.env*`, local configs, secrets, native config files), installed dependencies, and free ports, so it fails to run. ehai must make every worktree runnable automatically.

**On worktree creation, run an env pipeline (each step logged and resumable):**

1. **Discover required files.** Union of:
   - gitignored files in the source checkout matching known patterns: `.env`, `.env.*` (not `.env.example`), `*.local.*`, `config/local*`, `.npmrc`, `.yarnrc.yml`, `local.properties`, `google-services.json`, `GoogleService-Info.plist`, `app.config.local.*`, `.python-version`, `.tool-versions`, `docker-compose.override.yml`, `.vscode/settings.json` (optional)
   - user-configured include/exclude globs in `[env]`
   - files referenced by `.env.example` / `.env.sample` keys

   Never copy `node_modules`, `.venv`, build outputs, or `.git`.
2. **Materialize.** Copy (default) or symlink (`env.strategy = "copy" | "symlink"`). Copy is safer, since a POC change won't leak back.
3. **Validate.** Compare keys in `.env.example` with the materialized `.env*`. Report missing keys and ask via the question UI (provide the value now / skip / use a mock value).
4. **Port isolation.** Allocate unique free ports per worktree (`PORT`, `VITE_PORT`, Next dev port, Metro `RCT_METRO_PORT`/`--port`, Expo port, Go/Python API ports). Write them to an overlay file (`.env.ehai.local`) and inject them into run commands. Keep a port registry at `~/.ehai/ports.json` and release ports on discard. Set a unique `COMPOSE_PROJECT_NAME` for docker compose.
5. **Install dependencies** (configurable per stack, cached, can be skipped):
   - JS: detect the package manager by lockfile (pnpm / yarn / npm / bun). Prefer the pnpm store or hardlink strategies for speed. Optional `node_modules` reuse via a reflink/hardlink copy when the lockfile is identical.
   - Go: `go mod download`.
   - Python: detect uv / poetry / pipenv / pip+requirements; create a per-worktree venv.
   - RN: `pod install` and Gradle are **off by default** (slow); opt-in via config.
6. **Health check.** Run the configured smoke command (e.g. type-check or start the dev server and poll the port). Report "ready" or exactly what's broken, and suggest a fix.
7. **Safety.** Make sure materialized env files are gitignored in the worktree; refuse to stage them. Never send env contents to the tracker, logs, or the LLM (values redacted, keys only).

**Commands:** `ehai env sync [path]` · `ehai env check` · `ehai env doctor`. The same capabilities are exposed as MCP tools.

**Config example**
```toml
[env]
strategy = "copy"
include = ["apps/*/.env*", "services/**/config.local.yaml"]
exclude = [".env.production"]
install_deps = true
rn_native_install = false
smoke = { web = "pnpm -C apps/web typecheck", api = "go build ./..." }
```

---

## 7. Asking the user (native question UI everywhere)

- Every slash-command template must instruct the agent: *"When you need input from the user, ask with your harness's multiple-choice question tool (e.g. `AskUserQuestion` in Claude Code). Offer 2–4 concrete options, recommended first. If no such tool exists, present a numbered list and wait."*
- The engine exposes **MCP elicitation** for clients that support it. For clients that don't, MCP tools return a structured `needs_input` result `{question, header, options[], multi_select}` that the command template renders through the harness's question UI.
- Batch related questions (max 4 per round). Never ask what can be detected from the repo or config.

---

## 8. Understanding and adapting to users (`ehai-prefs`) — a core feature

**Goal:** ehai continuously learns how each user and each repo works, and adapts its questions, defaults, and outputs. This works at **two levels**:

### 8.1 Two levels: global (user) and repo (project)
- **Global user profile** (`~/.ehai/prefs/user.toml`): role, communication style (plain language vs technical, detail level), visual preferences, planning style, POC taste, which questions they always answer the same way.
- **Repo profile** (`<repo>/.ehai/profile.toml`; config decides whether it is committed):
  - **Detected conventions:** folder structure, naming, component patterns, state management, API layer, test framework, lint/format rules, commit-message style, branch naming, PR template, label set.
  - **Team decisions:** e.g. "use bottom sheets on mobile", "story points are Fibonacci".
  - Committing the repo profile lets the whole team share learned conventions. Personal observations stay in a gitignored `.ehai/prefs/project.local.toml`.
- **Resolution order:** explicit repo team decision > explicit user preference > observed repo convention > observed user preference > defaults.
- **Refresh:** repo conventions are re-detected incrementally when relevant files change (hash-based).

### 8.2 Role awareness
- Role (`pm` / `dev` / `both`) is set at init and can be overridden per session.
- **PM output** (Claude desktop **or Claude Code CLI**):
  - plain language, no jargon, short
  - visuals first: Mermaid diagrams, tables, and an auto-generated HTML report opened in the browser when on CLI
  - always ends with clear next options presented through the question UI
- **Dev output:** concise and technical, with file paths, diffs, and commands.
- The same slash commands work for both roles; templates branch on role. PM commands must work fully inside Claude Code CLI (e.g. `/ehai:plan` in a terminal renders the PRD and opens the visual report).

### 8.3 Learning
The rest of this section describes how preferences are captured and used.

- **Stores:** global `~/.ehai/prefs/user.toml` + per-project `.ehai/prefs/project.toml` (gitignored by default).
- **Signals captured** (observed behaviour only, never PHI):
  - POC choices: which POC is liked or promoted, what gets composed from which, recurring feedback themes ("denser tables", "less color", "bottom sheets over modals")
  - Planning style: task granularity, acceptance-criteria format, story wording, estimate scale
  - Answers to repeated questions, so ehai can stop asking them
- **Preference record:** `{key, value, scope, evidence[], confidence, source: explicit|observed, first_seen, last_seen}`. Explicit statements win over observations. Confidence decays if a preference is not reinforced, and contradicting evidence lowers it.
- **Use:** exposed as an MCP resource `ehai://prefs` and injected into POC/planning prompts as a compact "User preferences" block. High-confidence preferences become defaults (still shown, still overridable).
- **Control:** `/ehai:prefs` shows them in a readable table; the user can edit, pin, or delete any entry, or reset. Learning can be disabled entirely: `prefs.learning = false`.

---

## 9. Compliance guard (`ehai-guard`) — configurable

- `compliance.mode = "off" | "warn" | "strict"` (choose during `ehai init`, change anytime).
- **PHI/PII detector:** pluggable rules. Defaults include SSN, MRN-like IDs, dates of birth, phone, email, address, insurance/member IDs, plus a heuristic for name + medical-term combinations. Org-specific regexes can be added in config.
- **Applied to:** tracker writes (titles, bodies, comments), PRDs, POC fixtures/mock data, logs, reports.
  - `warn`: show findings and ask (redact / keep / cancel).
  - `strict`: block and auto-redact, and force synthetic data in POCs.
- **Audit log:** append-only JSONL at `~/.ehai/audit/` (what was written where, by which command; never the sensitive values).
- **No telemetry.** No network calls except GitHub/tracker APIs and update checks (update checks can be disabled).

---

## 9A. Sandboxing and safety (configurable)

- **Setting:** `sandbox.mode = "off" | "restricted" | "container"`. The default is `restricted`.
- **`restricted`:**
  - POC agents and commands may write only inside their worktree and `~/.ehai/`.
  - Commands run with an allowlist (package managers, dev servers, git read operations, test runners). Destructive commands (`rm -rf` outside the worktree, `git push`, `git reset` on the source checkout, force operations) are blocked.
  - Network access is limited to package registries and GitHub where the OS supports it.
  - **Implementation:**
    - Linux: Landlock (`landlock` crate), with `bubblewrap` if installed.
    - macOS: `sandbox-exec` profiles.
    - Windows: a restricted job object, at minimum.
    - Always: path-based enforcement inside ehai itself.
- **`container`:** run POC dev servers and installs in Docker/Podman (or a devcontainer if the repo has one), with the worktree mounted. Only used if a container runtime is detected.
- **Harness-level safety:** where a harness supports its own sandbox or permission settings (e.g. Claude Code sandbox/permissions in `settings.json`), the adapter writes worktree-scoped settings for POC sessions: allow edits inside the worktree, deny pushes to the default branch, deny reads of real env secrets by the model. Verify each harness's current settings format in its docs.
- **Fallback:** if a sandbox feature is unavailable on a platform, `ehai doctor` reports it and ehai falls back to path enforcement with a clear warning. It never fails silently.
- **Tests:** blocked-command tests and an escape-attempt test suite (path traversal, symlink escapes).

---

## 10. Harness adapters (`ehai-adapters`)

For each harness: **detect → generate → install → verify → uninstall**. Requirements:
- Idempotent: running twice changes nothing.
- Edits to shared config files go inside marked blocks (`# >>> ehai >>>` … `# <<< ehai <<<`) or exact JSON keys, with a backup taken before the first edit.
- Fully reversible with `ehai uninstall`.

| Harness | MCP registration | Commands / skills |
|---|---|---|
| Claude Code | user-scope MCP (`claude mcp add --scope user` or config file) | Claude Code **plugin** (commands + skills), installable from a local marketplace |
| Claude desktop | `claude_desktop_config.json` | skills for the PM flows (plan, breakdown, status, triage, prefs) |
| Codex CLI | `~/.codex/config.toml` `[mcp_servers]` | custom prompts dir |
| Gemini CLI | `~/.gemini/settings.json` `mcpServers` | `~/.gemini/commands/*.toml` |
| Cursor | `~/.cursor/mcp.json` | user commands |
| OpenCode | its config | its commands dir |
| VS Code / Copilot | user `mcp.json` | prompt files |
| Windsurf | its MCP config | workflows |
| Any other | — | global `AGENTS.md` snippet describing the commands and tools |

(Verify every path and format against current docs before implementing; see section 0.)

- Templates live in `templates/`, rendered with brand name/prefix, and are snapshot-tested per harness.
- `ehai adapters list|install|remove|regenerate` — `regenerate` runs automatically after updates and name changes.

---

## 11. Updates and reliability (`ehai-update`)

- **Private distribution:** binaries are published as releases in the client's **private GitHub repo** (macOS arm64/x64, Linux x64/arm64 musl static, Windows x64), each with `SHA256SUMS` + a minisign signature.
- **One-line install** (auth via `gh` or a token):
  ```bash
  curl -fsSL -H "Authorization: Bearer $(gh auth token)" \
    https://raw.githubusercontent.com/<ORG>/ehai/main/install/install.sh | sh
  ```
  If `gh` isn't installed, the script falls back to `GITHUB_TOKEN`. The PowerShell equivalent is `install.ps1`. The script installs to `~/.ehai/bin`, adds it to PATH, then runs `ehai init`.
- **`ehai update`:**
  1. Download to staging.
  2. Verify checksum and signature.
  3. Keep the current binary as `ehai.prev`.
  4. Atomic swap.
  5. Run `ehai doctor --post-update`.
  6. If the doctor fails, roll back automatically.

  `ehai update --rollback` rolls back manually. Release channels: `stable` and `beta`. The background update check (at most once per day) only notifies; it never auto-installs unless `update.auto = true`.
- **Never break the user:**
  - Config has a `schema_version`. Migrations are forward-only, pure, and property-tested; the old config is backed up before migrating.
  - MCP tools: only additive changes within a major version; deprecated tools keep working for one major version.
  - Adapters are regenerated after each update; user edits outside marked blocks are preserved.
- **Always running:**
  - The MCP server is spawned per session over stdio, so there is no long-lived daemon to crash.
  - Panics are caught at the tool boundary and returned as structured errors; the server never dies on a bad request.
  - Every external call has timeouts and retries with backoff.
  - `ehai doctor` checks binary, config, auth, tracker reachability, adapters, git version, and ports, and auto-fixes what it safely can.
- **Concurrency:** file locks (`fs4`) on config, manifest, and port registry, since several harness sessions may run at once.

---

## 11A. Versioning, CI and releases

- **Versioning:** SemVer. The first release is **`0.1.0-alpha.1`**; set this in every crate (use `version.workspace = true`). `ehai --version` prints version, commit, build date, and target.
- **CI (`.github/workflows/ci.yml`) runs only on code changes.**
  - Triggers: `push` and `pull_request`, filtered with `paths:` to `crates/**`, `templates/**`, `install/**`, `tests/**`, `Cargo.toml`, `Cargo.lock`, `deny.toml`, `rust-toolchain.toml`, `.github/workflows/**`.
  - Doc-only changes (`docs/**`, `**/*.md`, `LICENSE`, images) do **not** trigger CI.
  - Because required status checks can block doc-only PRs, add a tiny `ci-skip.yml` with the inverse path filter that reports the same check names as passing, or use a single `changes` job (`dorny/paths-filter`) that gates the heavy jobs. Pick one and document it in `DECISIONS.md`.
  - **Jobs:**
    - `fmt` and `clippy -D warnings`
    - `nextest` on Linux, macOS, and Windows
    - `cargo-deny`
    - coverage (Linux)
    - template snapshot check
  - Use `concurrency` to cancel superseded runs, and `Swatinem/rust-cache` for caching.
- **Release (`.github/workflows/release.yml`) runs only on tags** matching `v*.*.*` and `v*.*.*-*` (e.g. `v0.1.0-alpha.1`). It:
  1. Verifies the tag equals the workspace version, and fails if not.
  2. Cross-compiles all targets (`cargo-zigbuild` or `cross` for Linux musl; native runners for macOS and Windows).
  3. Produces archives, `SHA256SUMS`, and a minisign signature.
  4. Creates a GitHub Release in the private repo with generated notes from conventional commits (`git-cliff`). Tags with a pre-release suffix (`-alpha`, `-beta`, `-rc`) are marked **pre-release**.
- **Update channels:** pre-release tags map to the update channels (`alpha` → `beta` → `stable`). `ehai update` only offers versions on the user's channel; the default channel during 0.x is `alpha`.
- **Conventional commits:** enforce them with a lightweight commit-msg check documented in `CONTRIBUTING.md`. A `just release 0.1.0-alpha.2` recipe bumps the version, updates the changelog, commits, and tags.

---

## 12. Milestones (do them in order)

- **M0 – Skeleton:** workspace at version `0.1.0-alpha.1`, path-filtered CI + tag-only release workflow (section 11A), `ehai --version`, config load/merge + schema v1, logging, brand name support, `Prompter` trait + the cliclack-based UI kit, `ehai doctor` stub.
- **M1 – MCP + Claude Code + init wizard:** MCP server with a `ping` tool and a prefs resource, Claude Code plugin adapter, native-question pattern in templates, the full interactive `ehai init` wizard (auto-detect, summary card, minimal questions, live checklist), repo onboarding, role selection.
- **M2 – Tracker:** `Tracker` trait + GitHub Projects v2 adapter + contract tests (wiremock), `tracker set`, `ensure_schema`, idempotent sync engine.
- **M3 – PM flow:** plan / breakdown / sync / status / triage (MCP tools + command templates + Claude desktop skills), guard integrated.
- **M4 – Env provisioning:** `ehai-env` complete, with integration tests on fixture repos (RN Expo, Vite React, Next.js, Go, FastAPI, monorepo).
- **M5 – POC engine:** stack detection, design-token extraction (web + RN), worktree lifecycle, manifest, preview + screenshots, new/iterate/compare/discard.
- **M6 – Compose + promote + adaptation:** compose with lineage, promote via PR, global + repo profiles, convention detection, role-aware outputs (PM in Claude desktop and Claude Code CLI), preference learning end-to-end, sandboxing (section 9A).
- **M7 – All harness adapters:** Codex, Gemini CLI, Cursor, OpenCode, VS Code, Windsurf, AGENTS.md; uninstall; regenerate.
- **M8 – Distribution:** install scripts, release workflow (cross-compile, checksums, signing), self-update + rollback, migration tests.
- **M9 – Hardening:** e2e suite, docs (`README`, PM quick-start with screenshots, dev guide, config reference, troubleshooting), performance pass.

---

## 13. Testing requirements

- **Unit tests** for every module; **coverage ≥ 80%** on core, env, poc, tracker, prefs, guard, update.
- **Git integration tests** on temp repos. Assert invariants:
  - the default branch, HEAD, and working tree of the source checkout are byte-identical before and after every POC operation
  - env files are never staged
  - ports never collide across parallel worktrees
- **Tracker contract tests:** one shared suite run against every adapter (mocked HTTP), plus an opt-in live test behind `EHAI_LIVE_GH=1`.
- **Snapshot tests (`insta`)** for every generated adapter file and slash-command template, per harness and per brand name.
- **Property tests:** config migrations (any v1 config → latest is valid and preserves user values), PHI detector (no false negatives on generated PHI samples), port allocator.
- **MCP protocol tests:** start the server, call each tool over stdio, validate schemas; malformed input must never crash the server.
- **Update tests:** simulated failed post-update doctor → rollback restores the previous binary and config.
- **E2E (`tests/e2e`):** install from a local release → init (non-interactive flags) → sync to a mocked tracker → create POC on a fixture RN app and a fixture Vite app → compose → discard → uninstall leaves no traces.
- **Fixtures:** small sample repos for RN Expo, bare RN, Vite React, Next.js, Go API, FastAPI, and a pnpm monorepo, all with synthetic data only.

---

## 14. Definition of done

- One-line install works on macOS, Linux, and Windows from the private repo.
- After `ehai init`, slash commands work in Claude Code and at least two other harnesses; the PM flows work in Claude desktop.
- PM can go from idea → PRD → approved breakdown → GitHub Project items without touching a terminal command beyond setup.
- POCs run out of the box in a fresh worktree (env, deps, ports handled), match the app's design system, iterate and compose, and never touch main until promoted.
- Preferences visibly change later outputs and are fully user-editable.
- Updating never leaves a broken install; rollback is proven by tests.
- Setup feels smooth and guided: a new user finishes `ehai init` in under 2 minutes, answering at most about 5 questions.
- A PM can run the full planning flow from either the Claude desktop app or the Claude Code CLI.
- ehai's outputs measurably follow learned global and repo preferences.
- Doc-only commits trigger no CI. Pushing tag `v0.1.0-alpha.1` produces a signed pre-release.
