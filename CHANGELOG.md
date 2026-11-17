# Changelog
## 0.1.0-alpha.1 — 2026-11-17

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

### Docs
- Roadmap to 0.1.0-alpha and the approved roles/team plan
- Mark roles/team plan approved
- Resolve D1/D2 — release repo stays public, minisign key is in place

### Chores
- M0 skeleton — workspace, config v1, logging, UI kit, doctor stub, CI
