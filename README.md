# ehai

AI-SDLC ops engine for health-tech teams:
- PMs plan requirements and push them to GitHub Projects.
- Developers carry tasks through the dev lifecycle from any AI coding harness.
- Anyone can generate isolated UI/UX proof-of-concepts that match the app's design system.

> **Status:** `0.1.0-alpha.1`, M0 skeleton. See [docs/PROGRESS.md](docs/PROGRESS.md).

## Build from source
```bash
cargo build --release
./target/release/ehai --version
./target/release/ehai doctor
```

## Commands (so far)
| Command | What it does |
|---|---|
| `ehai doctor` | Health checks, each failure with a fix hint |
| `ehai config show` | Effective merged config |
| `ehai config path` | Global and repo config locations |
| `ehai config schema` | JSON schema for `config.toml` |

Config precedence is: defaults < `~/.ehai/config.toml` < `.ehai.toml` (repo) < `--config` < `EHAI_SECTION__KEY` env < CLI flags.

Docs: [DECISIONS](docs/DECISIONS.md) · [CONTRIBUTING](CONTRIBUTING.md) · [spec](CLAUDE.md)
