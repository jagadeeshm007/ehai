# ehai v0.1.0-alpha.2 — Planning Document

> Status: **DRAFT** — work on this after alpha.1 is validated in real use.
> Do not start implementation until alpha.1 is tagged, pushed, and confirmed working end-to-end.

---

## Goal

alpha.2 turns ehai from a fully functional but rough-edged alpha into a tool teams
can adopt at scale. The theme is **reach and reliability**: more trackers, AI-driven
server-side operations, a web status dashboard, a plugin SDK, and performance work
that makes the POC engine feel instant on large repos.

---

## Milestone structure

| Milestone | Theme | Size |
|---|---|---|
| M-A1 | Linear & Jira tracker adapters | M |
| M-A2 | Server-side AI operations | L |
| M-A3 | CI/CD integration | M |
| M-A4 | Web status dashboard | M |
| M-A5 | Plugin SDK | L |
| M-A6 | Monorepo & performance | M |
| M-A7 | PR review assistance | S |
| M-A8 | GitHub App mode | L |
| M-A9 | Hardening & docs | M |

---

## M-A1 — Linear & Jira tracker adapters

**Problem:** GitHub Projects is the only real tracker today (local is a stub). Teams on Linear or Jira can't use the PM flow.

**Scope:**

- `ehai-tracker`: implement `LinearAdapter` using Linear's GraphQL API
  - `ensure_schema`: create Status, Priority, Size, Type labels/states
  - `create_item`, `update_item`, `set_status`, `comment`, `link_pr`, `get_item`, `list_items`
  - Team: read Linear team members (use for `team` tool)
  - `ehai-id`: embed in issue description footer (Linear supports markdown)
- `ehai-tracker`: implement `JiraAdapter` using Jira REST v3
  - Similar contract, field mapping to Jira's custom fields
- `ehai init` wizard: add Linear/Jira options in tracker selection
- `ehai tracker set linear` / `ehai tracker set jira`
- Shared contract test suite (already defined in `ehai-tracker/tests/`) runs against all three adapters via wiremock

**Config:**
```toml
[tracker]
type = "linear"
linear_api_key = ""   # read from EHAI_TRACKER__LINEAR_API_KEY env
team_id = ""

# or
type = "jira"
jira_base_url = "https://myco.atlassian.net"
jira_email = ""
jira_api_token = ""   # read from EHAI_TRACKER__JIRA_API_TOKEN env
project_key = "MYCO"
```

**Tests:** wiremock-based contract tests for all three adapters sharing the same test suite. Live test flags: `EHAI_LIVE_GH=1`, `EHAI_LIVE_LINEAR=1`, `EHAI_LIVE_JIRA=1`.

**ADR needed:** field mapping strategy for Linear/Jira vs GitHub Projects field names.

---

## M-A2 — Server-side AI operations

**Problem:** Right now all reasoning happens inside the user's harness. ehai has no way to do AI-driven work (PRD generation, triage classification, gap finding) when triggered by a webhook or a CLI command without an active harness session.

**Scope:**

- `ehai-core`: add `[ai]` config section:
  ```toml
  [ai]
  provider = "anthropic"       # anthropic | openai | azure | local
  model    = "claude-sonnet-4-6"
  api_key  = ""                # read from EHAI_AI__API_KEY env
  base_url = ""                # for openai-compatible endpoints
  ```
- `ehai-ai` crate (new): thin async client over `reqwest`, wrapping the provider's chat completion API. Supports streaming. `thiserror` errors. No LLM SDK dependency — hand-rolled to avoid OpenSSL and bloat.
- MCP tools that today require a harness-driven reasoning loop get a `--server-side` mode:
  - `save_prd --server-side`: ehai calls the LLM directly to produce the PRD from a description; no harness needed
  - `triage --server-side`: ehai classifies type/priority/size without a harness reasoning loop
  - `plan_explore --server-side`: ehai runs the gap check autonomously
- New CLI commands:
  - `ehai plan "<description>"` — full PRD generation from the terminal, no harness
  - `ehai triage "<report>"` — triage from the terminal
- `ehai-ai` is off by default; enabled when `[ai]` config is set
- Keychain storage for the API key (same `keyring` crate as GitHub token)

**Security:** API key never logged, never sent to the tracker, never in audit log. TLS enforced (rustls, no OpenSSL).

**Tests:** wiremock-based for the AI client. Property tests for token counting / context window enforcement.

**ADR needed:** provider abstraction strategy (trait-based vs enum dispatch).

---

## M-A3 — CI/CD integration

**Problem:** Developers check CI status by going to GitHub. Tracker cards don't reflect CI state automatically.

**Scope:**

- `ehai-ci` crate (new): read GitHub Actions run status and artifact URLs via REST API
  - `list_runs(pr_number)`, `get_run(id)`, `watch_run(id, timeout)` (long-poll)
- New MCP tools:
  - `ci_status`: return pass/fail/running for the CI runs linked to an item's PR
  - `ci_watch`: block until CI completes; return result (use in `/ehai:pr` flow)
- `/ehai:pr` updated: after PR creation, calls `ci_watch` and reports CI result when it lands
- `/ehai:done` updated: blocks if CI is still running; reports failure before moving card to Done
- Tracker auto-update: when CI fails on a PR linked to a card, add a comment and optionally move card back to In Progress (configurable: `ci.auto_update_on_failure = true`)
- Webhook mode (opt-in): `ehai serve --webhook` starts a tiny HTTP server that accepts GitHub webhook events and updates cards in real time — useful in CI environments
- Config:
  ```toml
  [ci]
  provider = "github-actions"   # github-actions | (more later)
  auto_update_on_failure = false
  ```

**Tests:** wiremock for the GitHub Actions API. Integration test: mock PR with mock CI run → auto-update fires.

---

## M-A4 — Web status dashboard

**Problem:** PMs want a visual board without leaving the browser. The terminal status report is text-only.

**Scope:**

- `ehai serve` command: starts a lightweight local HTTP server (default port 8080)
- Serves a single-page HTML dashboard:
  - Epics + story progress (% bar per epic)
  - Kanban board view (Status columns)
  - Stale items highlighted
  - Open PRs with CI status badge
  - Risk list
- Tech: server-side rendered HTML via `askama` templates + `axum`. No frontend build step, no npm. CSS inline or served as a static file.
- Auto-opens in the browser (`open`/`xdg-open`/`start`)
- Refreshes every 30 seconds via a simple `<meta refresh>` or SSE
- `/ehai:status --html` already exists — this replaces the static HTML output with a live-updating local server
- Config:
  ```toml
  [dashboard]
  port = 8080
  open_browser = true
  ```
- `ehai serve --stop` to kill it

**Tests:** `reqwest` integration tests against the running server. Snapshot tests for the HTML output.

**ADR needed:** axum vs actix-web; askama vs minijinja for templating.

---

## M-A5 — Plugin SDK

**Problem:** Teams want to extend ehai with custom MCP tools (e.g. a tool that checks their internal deployment system, or a custom compliance check).

**Scope:**

- Plugin protocol: a plugin is an executable that ehai spawns and talks to over a simple JSON-lines protocol (not MCP — simpler). ehai proxies the plugin's tools into its own MCP server as first-class tools.
- `ehai plugin install <path>` — register a plugin binary
- `ehai plugin list` — list installed plugins
- `ehai plugin uninstall <name>` — unregister
- Plugin manifest (JSON in the plugin binary's `--manifest` output):
  ```json
  {
    "name": "acme-deploy",
    "version": "1.0.0",
    "tools": [
      {
        "name": "deploy_check",
        "description": "Check if a task is cleared for deployment",
        "params": { "item": "string" }
      }
    ]
  }
  ```
- Plugin sandbox: plugins run in `restricted` mode by default (Landlock/sandbox-exec)
- SDK helpers: a small Rust crate (`ehai-plugin-sdk`) with the JSON-lines protocol types and a derive macro, so plugin authors can focus on the tool logic
- Example plugin: `examples/acme-deploy` (a minimal plugin that always returns `{"status":"ok","cleared":true}`) — doubles as the SDK integration test

**Tests:** spawn the example plugin, call its tool via the proxied MCP, verify response. Escape-attempt test: plugin tries to write outside its allowed path → blocked.

---

## M-A6 — Monorepo & performance

**Problem:** POC provisioning on large monorepos (pnpm workspaces with 50k files) takes 10–30 seconds for `node_modules` linking. Stack detection is slow on deep directory trees.

**Scope:**

- **Parallel worktree provisioning:** provision multiple POC worktrees in parallel using `tokio::spawn`. Port allocation is already lock-safe (fs4). The bottleneck is dependency install — make it concurrent per worktree.
- **`node_modules` hardlink copy:** when the lockfile is identical between source and worktree, hardlink individual files instead of running `npm install` again. `ehai-env` already has the detection logic; implement the hardlink copy using `std::fs::hard_link` with a fallback to full copy.
- **Incremental design-token extraction:** `ehai-stack` currently re-reads all theme files on every POC. Cache the result in `.ehai/cache/design-profile.json` keyed by file hashes. Invalidate only the files that changed.
- **Stack detection parallelism:** `ehai-stack`'s detection runs checks sequentially. Parallelize them with `tokio::join!`.
- **Monorepo workspace scoping:** `ehai repo init` on a pnpm/yarn/Nx monorepo should ask which workspace(s) to treat as "this project" and store that in `.ehai/profile.toml`. POC commands default to the workspace scope, not the repo root.
- **`ehai env doctor --benchmark`:** report provisioning time per step so users can see which step is slow.

**Targets:**
- POC provisioning on a cold monorepo: under 60 seconds
- POC provisioning when lockfile matches: under 5 seconds (hardlink path)
- Design-token extraction on a cached run: under 500ms

**Tests:** fixture repos with synthetic large `node_modules` (use symlinks in tests to avoid actual size). Benchmark tests (run separately, not in `nextest --workspace`).

---

## M-A7 — PR review assistance

**Problem:** Developers want ehai to help review incoming PRs against the task's acceptance criteria.

**Scope:**

- `/ehai:review [PR number or URL]`: pulls the PR diff and the linked task's acceptance criteria, generates a structured review comment
  - For each acceptance criterion: does the diff address it? (yes / no / partial)
  - Flags obvious bugs or missing error handling (not a full code review — focused on the task's scope)
  - Posts the review as a draft GitHub review or a comment
- MCP tool: `review_pr(pr: string, post: bool)` — `post: false` returns the review text; `post: true` posts it to GitHub
- `/ehai:start` updated: when starting a task, offer to `watch` the linked PR for new reviews and surface them in the harness

**Tests:** wiremock for GitHub PR diff and review APIs. Unit tests for the criterion-matching logic.

---

## M-A8 — GitHub App mode

**Problem:** Per-user OAuth tokens require every team member to run `ehai init`. GitHub Apps can be installed org-wide, so the whole team shares a single authenticated integration.

**Scope:**

- Register `ehai` as a GitHub App (documentation + setup guide — the actual App registration is a one-time manual step by the user)
- `ehai-tracker/github`: support App JWT auth in addition to personal token auth
  - `AppAuth { app_id, private_key_pem }` variant in the auth config
  - JWT generation: `jsonwebtoken` crate
  - Installation token: exchange JWT for a short-lived installation token every hour
- `ehai init` wizard: add "GitHub App" option in the GitHub auth step
- Config:
  ```toml
  [github]
  auth = "app"
  app_id = 12345
  private_key_path = "~/.ehai/github-app.pem"   # gitignored, never committed
  installation_id = 67890
  ```
- `ehai doctor` checks App auth validity and token expiry

**Tests:** wiremock for JWT exchange and installation token refresh. Property test for JWT generation (clock drift handling).

---

## M-A9 — Hardening & docs

- **E2E test suite expansion:** cover the Linear adapter, CI integration, dashboard, and plugin SDK with the existing e2e fixture infrastructure
- **Coverage enforcement:** add `cargo-llvm-cov` to CI for `ehai-ai`, `ehai-ci`, `ehai-stack` (cached path), plugin SDK
- **Docs site updates:** alpha.2 feature guides (AI config, Linear/Jira setup, plugin authoring, monorepo setup)
- **Performance dashboard in docs:** documented benchmarks with target numbers
- **`ehai doctor` expanded:** checks for AI API key validity, CI webhook URL reachability, plugin binary health
- **Migration from alpha.1:** any config changes are backward-compatible; migration documented in CHANGELOG

---

## Open questions (to resolve before starting implementation)

1. **Linear API key vs OAuth:** Linear supports both. OAuth gives per-user tokens; API key is simpler but org-wide. Which should be the default?

2. **Server-side AI — model selection:** should ehai default to the cheapest model for classification tasks (triage, gap-finding) and a smarter model for PRD generation? Or leave it to the user's config?

3. **Plugin language:** the plugin protocol is language-agnostic (JSON-lines over stdio), but should we also ship a TypeScript SDK for plugin authors who don't use Rust?

4. **Dashboard auth:** the local web dashboard has no auth (localhost only). Is this acceptable, or should it bind to `127.0.0.1` only and optionally require a one-time token?

5. **GitHub App registration:** should we ship a pre-registered ehai GitHub App (users install it on their org), or instruct users to create their own App? Pre-registered is smoother but requires maintaining the App; user-created Apps are more private.

---

## Definition of done for alpha.2

- Linear and Jira tracker adapters pass the shared contract test suite with wiremock
- `ehai plan "<description>"` works from the terminal without a harness when `[ai]` is configured
- `ehai serve` opens a live dashboard in the browser with auto-refresh
- A plugin written with `ehai-plugin-sdk` registers, calls a custom tool, and runs sandboxed
- POC provisioning on a large repo with an unchanged lockfile completes in under 5 seconds
- `/ehai:review` posts a structured review comment to a real GitHub PR (manual smoke test)
- All new code meets the same test coverage and clippy standards as alpha.1
- CHANGELOG.md updated, `v0.1.0-alpha.2` tagged, release workflow green

---

## Estimated build order

1. M-A1 (Linear/Jira) — highest user value, unblocks teams not on GitHub Projects
2. M-A2 (server-side AI) — enables M-A3, M-A7, and future automation
3. M-A3 (CI integration) — builds on M-A2's AI client + existing tracker
4. M-A6 (performance) — fastest value-to-effort ratio; unblocks large-monorepo users
5. M-A5 (plugin SDK) — enables the ecosystem
6. M-A4 (dashboard) — PM-facing polish
7. M-A7 (PR review) — developer-facing polish
8. M-A8 (GitHub App) — org-scale deployment
9. M-A9 (hardening) — always last
