# Changelog
## 0.1.0-alpha.2 — 2026-11-19

### Features
- **update:** Private-repo support — EHAI_TOKEN, API asset URL, token persistence
- **install:** Pick the most stable release by default, add EHAI_VERSION

### Fixes
- **audit:** Alpha.1 E2E audit — 5 fixes from full audit run
- **hooks:** Remove --quiet from nextest (not supported)
- **install:** Stop requiring minisign to be preinstalled
- Remove hidden runtime/install dependencies found in audit

### Docs
- Fix site nav, section separation, ToC clutter, and link affordance
- Add changelog, fix spacing, change logo icon
- **setup:** Add Removing ehai section — disconnect, repo removal, full uninstall
- Add how-it-works page, colored diagrams, copy-markdown button

### Chores
- **dev:** Pre-commit/pre-push hooks; docs-update rule in CLAUDE.md
## 0.1.0-alpha.1 — 2026-11-18

### Features
- M1 — MCP server, Claude Code plugin adapter, init wizard, repo onboarding
- **cli:** Self install, connect/disconnect, one-command uninstall
- M2 — tracker trait, GitHub Projects v2 adapter, idempotent sync engine
- M3 - PM flow (plan, breakdown, sync, status, triage), PHI guard, Claude desktop
- **roles:** Role tied to GitHub project admin, team cache, assign/mention
- **env:** Worktree provisioning — gitignored config, ports, deps, health check
- **poc:** M5 POC engine — isolated worktrees, design tokens, 6 MCP tools
- **m6:** Compose + promote + preferences + role-aware output
- **adapters:** M7 — all harness adapters (Codex, Gemini, Cursor, OpenCode, VS Code, Windsurf, AGENTS.md)
- **tracker:** Local JSON tracker + init gh-missing flow
- **update:** Self-update with verify + rollback, install scripts, release recipe
- Plan_explore loop, Desktop skills ZIP, device migration, pref observations
- Open-source generalization — MIT license, domain profiles, GitHub Pages docs

### Fixes
- **tracker:** Use `Work type` field (GitHub reserves `Type`); per-field schema errors
- **tracker:** Omit unchanged fields in updates; GitHub clears on explicit null
- Issues found by the live end-to-end run
- **cli:** Wire ehai migration, close M6 sandbox gap, fix dead explore check
- **deps:** Bump zip to 9.x (2 RUSTSEC fixes), trim its features, ignore chrono advisory
- **tests:** Make env install-skip and update fixtures platform-independent
- **ci:** Cargo-insta flag order, Windows snapshot path rendering; docs for M8
- **ci:** Add .gitattributes (root cause of Windows snapshot mismatch)
- **ci:** Normalize ehai.exe → ehai in help snapshot for Windows
- **ci:** Windows platform fixes in ehai-env tests
- **docs:** Remove custom_dir — overrides dir not committed (empty dirs not tracked by git)
- **release:** Remove staging dir before upload; use explicit archive glob for sha256sum
- Cargo fmt; fix SHA256SUMS path prefix (./ehai-* → ehai-* so install.sh grep works)

### Docs
- Roadmap to 0.1.0-alpha and the approved roles/team plan
- Mark roles/team plan approved
- Resolve D1/D2 — release repo stays public, minisign key is in place
- Comprehensive README, contributor setup, full MCP/skills/use-case docs, alpha.2 plan

### Chores
- M0 skeleton — workspace, config v1, logging, UI kit, doctor stub, CI
