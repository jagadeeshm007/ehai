# ehai

AI-SDLC ops engine for health-tech teams.

- **PMs** plan requirements and push them to GitHub Projects (or local storage without GitHub).
- **Developers** carry tasks through the dev lifecycle from any AI coding harness.
- **Anyone** generates isolated UI/UX proof-of-concepts that match the app's design system.

> **Status:** `0.1.0-alpha.1`, M8 complete — self-update with signature verification and automatic rollback, one-line install, all harness adapters, full POC engine, PM flow, env provisioning, roles/team, PHI guard. See [docs/PROGRESS.md](docs/PROGRESS.md).

---

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/jagadeeshm007/ehai/main/install/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/jagadeeshm007/ehai/main/install/install.ps1 | iex
```

Downloads the right binary for your OS/arch, verifies its checksum and minisign signature, installs to `~/.ehai/bin`, adds it to your PATH, and runs `ehai init`. The repo is public, so no authentication is required; `gh`/`GITHUB_TOKEN` are used when present only to avoid unauthenticated API rate limits. `EHAI_CHANNEL=beta|stable` picks a different release channel (default `alpha`); `EHAI_NAME=<name>` installs under a different command name.

### Install from source

```bash
cargo build --release
./target/release/ehai self install   # → ~/.ehai/bin/ehai + PATH entry
# open a new terminal
ehai init
```

### Updating

```bash
ehai update                 # check the configured channel, verify, swap in, auto-rollback if the post-update health check fails
ehai update --channel beta  # or alpha | stable, one-off override
ehai update --rollback      # restore the previous binary by hand
```

A background check (at most once a day) notifies you of a newer version without installing it; disable with `update.check = false` in config.

---

## Setup

```bash
ehai init
```

The wizard auto-detects OS, git, `gh` + auth, installed harnesses, Chrome, and the current repo. It asks at most 5 questions (role, harnesses, tracker, compliance mode, sandbox mode) and applies changes with a live checklist — including installing itself to `~/.ehai/bin` and adding that to your PATH, if it isn't already, so `ehai init` alone is enough even when run straight from a `cargo build` without a separate `self install` step.

**No `gh`?** The wizard detects this and asks: install `gh` to unlock GitHub Projects sync, or continue with local storage. Local storage supports all PM flows; GitHub sync can be added later with `ehai tracker set github`.

---

## Commands

### Setup and health

| Command | What it does |
|---|---|
| `ehai init` | Guided setup wizard |
| `ehai repo init` | Detect this repo's stack/conventions → `.ehai/profile.toml` |
| `ehai doctor` | Health checks; each failure includes a fix hint |
| `ehai doctor --post-update` | Post-update gate — a non-zero exit triggers automatic rollback |
| `ehai self install` | Copy this binary to `~/.ehai/bin` and add to PATH |
| `ehai update [--channel] [--rollback]` | Check, download, verify and swap in a newer release; roll back by hand |
| `ehai uninstall` | Remove harness integrations, PATH entry, and `~/.ehai` |

### Harness adapters

| Command | What it does |
|---|---|
| `ehai connect <id>` | Connect a harness: `claude-code`, `codex`, `gemini`, `cursor`, `vscode`, `opencode`, `windsurf`, `claude-desktop`, `agents-md` |
| `ehai disconnect <id>` / `--all` | Disconnect one or all harnesses |
| `ehai adapters list` | Show all adapters with detect / status |
| `ehai adapters install <id>` | Install one adapter |
| `ehai adapters remove <id>` | Remove one adapter |
| `ehai adapters regenerate` | Re-render all installed adapters (run after brand rename or update) |

### Tracker

| Command | What it does |
|---|---|
| `ehai tracker set` | Pick/create a tracker (`github` or `local`) |
| `ehai tracker status` | Show the connected project / local store |
| `ehai tracker schema` | Add missing fields to the GitHub project |

### PM workflow

| Slash command | Harness | What it does |
|---|---|---|
| `/ehai:plan` | Any | Interview → PRD (problem, users, goals, flows, metrics, risks) |
| `/ehai:breakdown` | Any | PRD → epics → stories → tasks with acceptance criteria |
| `/ehai:sync` | Any | Push breakdown to tracker idempotently |
| `/ehai:status` | Any | Progress report: % done per epic, blockers, stale items, open PRs |
| `/ehai:triage` | Any | Classify and de-duplicate a bug report → tracker item |
| `/ehai:assign` | Any | Assign a task to a team member |
| `/ehai:prefs` | Any | View / edit learned preferences |
| `/ehai:help` | Any | List all commands |

### Developer workflow

| Slash command | What it does |
|---|---|
| `/ehai:next` | Suggest the next task by priority and dependency |
| `/ehai:start #id` | Create branch, load task context, set In Progress |
| `/ehai:pr` | Draft PR description linked to issue + acceptance criteria |
| `/ehai:done` | Verify acceptance criteria and move card |

### POC engine

| Command / slash command | What it does |
|---|---|
| `/ehai:poc new "<idea>"` / `ehai poc new` | Create `poc1`: isolated worktree, design context, dev server |
| `/ehai:poc iterate <pocN> "<feedback>"` | Iterate → `pocN+1`, lineage recorded |
| `/ehai:poc compose "<take X from poc1, Y from poc2>"` | Synthesise a new POC from multiple parents |
| `/ehai:poc list` | Show all POCs for a feature |
| `/ehai:poc compare [pocA pocB …]` | Side-by-side screenshot gallery + diff summary |
| `/ehai:poc open <pocN>` | Start the POC's dev server |
| `/ehai:poc promote <pocN>` | Open a PR to the default branch (requires explicit confirmation) |
| `/ehai:poc discard <pocN>` | Archive as a git tag and remove the worktree |

POCs live outside the repo (`~/.ehai/worktrees/<repo-id>/<feature>/poc-<n>`), never touch `main`, and are automatically provisioned with the right `.env`, dependencies, and a unique port.

### Environment provisioning

| Command | What it does |
|---|---|
| `ehai env sync [path]` | Provision a worktree: copy env files, install deps, allocate ports, run smoke check |
| `ehai env check [path]` | Report missing `.env` keys vs `.env.example` |
| `ehai env doctor [path]` | Run smoke check only |

### Preferences and config

| Command | What it does |
|---|---|
| `ehai prefs list` | Show all learned preferences |
| `ehai prefs set <key> <value>` | Set a preference explicitly |
| `ehai prefs delete <key>` | Remove a preference |
| `ehai config show` | Effective merged configuration |
| `ehai config path` | Location of global and repo config files |
| `ehai config schema` | JSON schema for `config.toml` |
| `ehai role` | Show your current role and how it was resolved |
| `ehai team` | List the team (GitHub assignable users) |
| `ehai migration export [file]` | Bundle config, prefs and the local tracker into one ZIP |
| `ehai migration import <file> [--yes]` | Restore config, prefs and the local tracker on a new machine |

---

## Trackers

| Tracker | Setup | Features |
|---|---|---|
| `github` (default) | Requires `gh` or `GITHUB_TOKEN` with `project` scope | Full GitHub Projects v2 sync, PR linking, team, roles |
| `local` | No external dependencies | All PM flows, full offline support; no PR linking |

Switch: `ehai tracker set github` or `ehai tracker set local`.

GitHub Projects needs the `project` scope: `gh auth refresh -s project`.

---

## Harnesses

ehai works from any AI coding tool. After `ehai init`, slash commands are available immediately.

| Harness | Slash-command prefix | MCP config |
|---|---|---|
| Claude Code | `/ehai:` | user-scope MCP server + local plugin |
| Claude desktop | Prompts under `ehai` connector | `claude_desktop_config.json` |
| Codex CLI | `/ehai:` | `~/.codex/config.toml` + prompt files |
| Gemini CLI | `/ehai:` | `~/.gemini/settings.json` + TOML commands |
| Cursor | `/ehai:` | `~/.cursor/mcp.json` + rule files |
| OpenCode | `/ehai:` | `~/.config/opencode/config.json` + prompt files |
| VS Code / Copilot | `/ehai:` | user `mcp.json` (`servers` key) + prompt files |
| Windsurf | `/ehai:` | `~/.codeium/windsurf/mcp_server_config.json` + workflows |
| Any other | — | `~/AGENTS.md` reference card |

---

## Health-tech compliance

```toml
# ~/.ehai/config.toml
[compliance]
mode = "warn"   # off | warn | strict
```

- `warn`: surface PHI/PII findings (SSN, MRN, DOB, phone, email, insurance ID, name + medical term) and ask redact / keep / cancel before writing.
- `strict`: auto-redact High-confidence findings; report Low.
- Applies to: tracker writes, PRDs, POC mock data, reports, logs.
- Values are never logged — only kinds and counts (e.g. `mrn ×1, email ×1`).

---

## Configuration

Precedence (narrower wins): defaults → `~/.ehai/config.toml` → `.ehai.toml` (repo) → `--config` flag → `EHAI_SECTION__KEY` env → CLI flags.

```toml
# ~/.ehai/config.toml example
[brand]
name = "ehai"          # rename the binary and all slash commands

[compliance]
mode = "warn"

[sandbox]
mode = "restricted"    # off | restricted | container

[update]
channel = "alpha"      # alpha | beta | stable
auto = false
```

---

## Docs

- [docs/PROGRESS.md](docs/PROGRESS.md) — milestone log
- [docs/DECISIONS.md](docs/DECISIONS.md) — ADR log
- [CONTRIBUTING.md](CONTRIBUTING.md) — dev setup and commit conventions
- [CLAUDE.md](CLAUDE.md) — full product spec
