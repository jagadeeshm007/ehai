# ehai

AI-SDLC ops engine for health-tech teams:
- PMs plan requirements and push them to GitHub Projects.
- Developers carry tasks through the dev lifecycle from any AI coding harness.
- Anyone can generate isolated UI/UX proof-of-concepts that match the app's design system.

> **Status:** `0.1.0-alpha.1`, M1 (MCP server, Claude Code plugin, init wizard). See [docs/PROGRESS.md](docs/PROGRESS.md).

## Install from source
```bash
cargo build --release
./target/release/ehai self install   # → ~/.ehai/bin/ehai + PATH entry
# open a new terminal
ehai init
```

## Commands (so far)
| Command | What it does |
|---|---|
| `ehai init` | Guided setup: detects your tools, asks ≤ 5 questions, wires up Claude Code |
| `ehai repo init` | Detect this repo's stack/conventions → `.ehai/profile.toml` |
| `ehai connect <harness>` | Connect a harness (e.g. `claude-code`) |
| `ehai disconnect <harness>` / `--all` | Disconnect one or every harness |
| `ehai adapters list\|install\|remove\|regenerate` | Manage harness integrations |
| `ehai self install` | Put this binary on your PATH (`~/.ehai/bin`) |
| `ehai uninstall` | Remove ehai completely: harness integrations, PATH entry, `~/.ehai` |
| `ehai doctor` | Health checks, each failure with a fix hint |
| `ehai config show` | Effective merged config |
| `ehai config path` | Global and repo config locations |
| `ehai config schema` | JSON schema for `config.toml` |

Inside Claude Code: `/ehai:help`, `/ehai:prefs` (MCP server `ehai`, resource `ehai://prefs`).

Config precedence is: defaults < `~/.ehai/config.toml` < `.ehai.toml` (repo) < `--config` < `EHAI_SECTION__KEY` env < CLI flags.

Docs: [DECISIONS](docs/DECISIONS.md) · [CONTRIBUTING](CONTRIBUTING.md) · [spec](CLAUDE.md)
