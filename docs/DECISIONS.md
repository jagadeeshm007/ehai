# Decisions

ADR-style log. One entry per non-obvious decision. Newest last.

---

## ADR-0001 — Release repository
**Date:** 2026-11-17 · **Status:** accepted

Releases are published to the private repo **`jagadeeshm943/ehai`**. The owner/name appears in `Cargo.toml` (`workspace.package.repository`) and will be used by `install/` and `ehai-update` (M8). It can move to an org later; keep it in those few places.

## ADR-0002 — Create crates when a milestone needs them
**Date:** 2026-11-17 · **Status:** accepted

Spec §4 lists 12 crates. M0 creates only `ehai-core` and `ehai-cli`. Each remaining crate is added in the milestone that first puts code in it. Empty crates would add compile time, `Cargo.toml` noise and churn with nothing to test. The target layout in §4 is unchanged.

## ADR-0003 — Hand-rolled layered config instead of figment/config-rs
**Date:** 2026-11-17 · **Status:** accepted

- Each layer (global, repo, `--config`, env, CLI) is read as a raw `toml::Table`, deep-merged by a ~10-line recursive function (`ehai_core::config::deep_merge`), and deserialized once.
- This gives exact control over env mapping and migration-per-layer.
- It avoids a dependency whose provider model we'd fight.
- **Env mapping:** `EHAI_<SECTION>__<KEY>=value` maps to `section.key`. Only known top-level sections are mapped, so `EHAI_HOME`, `EHAI_NAME` and `EHAI_LIVE_GH` never leak into config. Values are parsed as TOML literals (`true`, `42`); anything else is a string.
- **Forward compatibility:** unknown keys are ignored on load, never rejected. Typed config is never written back. Only `migrate_file` rewrites a config file, and it works on raw tables, so unknown keys survive on disk.

## ADR-0004 — Prefer stdlib over small dependencies
**Date:** 2026-11-17 · **Status:** accepted

- File locking uses `std::fs::File::lock` (stable since 1.89) instead of `fs4`.
- The home directory comes from `std::env::home_dir` (behaviour fixed in 1.86) instead of `dirs`.
- Version metadata is a ~40-line `build.rs` (git sha, `SOURCE_DATE_EPOCH`-aware date, target) instead of `vergen`.
- Hence **MSRV = 1.89**. The toolchain is pinned to `stable` via `rust-toolchain.toml`.

## ADR-0005 — Doc-only changes: mirror workflow (`ci-skip.yml`)
**Date:** 2026-11-17 · **Status:** accepted

- `ci.yml` triggers only on code `paths:`.
- `ci-skip.yml` uses the identical list as `paths-ignore:` and defines jobs with the **same names** that just echo. Required status checks therefore pass on doc-only PRs, and doc-only commits run no real CI.
- We chose this over a `dorny/paths-filter` gate job because it literally meets "doc-only commits trigger no CI". A gate job would still start the CI workflow.
- **Caveat:** a PR that touches both docs and code runs both workflows. The real one is authoritative, so check names only go green when the real jobs pass.
- **Rule:** keep the two path lists in sync. Both files say so at the top.

## ADR-0006 — Terminal UI kit: cliclack behind a `Prompter` trait
**Date:** 2026-11-17 · **Status:** accepted

- All prompts go through `ui::Prompter`.
- `ClackPrompter` (cliclack) is used when stdin and stdout are TTYs and `--yes` is absent. `NO_COLOR` is honoured by `console`, the crate under cliclack. A NO_COLOR user on a TTY still gets interactive prompts, so it does not switch to plain mode.
- `PlainPrompter` is used for non-TTY or `--yes`. It accepts defaults and prints a plain transcript.
- The test fake is `PlainPrompter::scripted(answers)`, not a third implementation. Snapshots therefore show exactly what a CI user sees.
- `indicatif` and `ratatui` are deferred until a feature needs them (progress bars and the TUI views, M1+).

## ADR-0007 — Minimum supported OS versions
**Date:** 2026-11-17 · **Status:** accepted

| OS | Minimum | Notes |
|---|---|---|
| macOS | 12 | Release builds set `MACOSX_DEPLOYMENT_TARGET=12.0` |
| Windows | 10 1809 | Needed for ConPTY / modern terminal rendering |
| Linux | kernel 5.13 | Landlock available (sandbox §9A); musl static binaries run on any distro |

Older Linux kernels still run ehai. `ehai doctor` will report that Landlock is unavailable and fall back to path enforcement (M6).

## ADR-0008 — `EHAI_HOME` override
**Date:** 2026-11-17 · **Status:** accepted

`ehai_home()` honours `EHAI_HOME` before `~/.ehai`. Every test sets it to a tempdir, so tests never touch the real home directory. It also lets users relocate state.

## ADR-0009 — Release signing with a password-less minisign key
**Date:** 2026-11-17 · **Status:** accepted

- CI signs `SHA256SUMS` with minisign.
- The secret key (`minisign -G -W`, no password, so CI needs no TTY) is stored as the repo secret `MINISIGN_KEY`.
- The public key will be embedded in `ehai-update` (M8).
- The release job fails loudly if the secret is missing.

## ADR-0010 — Verified external versions
**Date:** 2026-11-17 · **Status:** accepted

Checked against crates.io and GitHub releases on 2026-11-17:
- Crates: clap 4.6, serde 1.0.229, toml 1.2, schemars 1.2, thiserror 2.0, tracing-subscriber 0.3.23, tracing-appender 0.2.5, cliclack 0.5.7, insta 1.49, proptest 1.12.
- GitHub Actions: checkout v7, upload-artifact v7, download-artifact v8, rust-cache v2, install-action v2, action-gh-release v3, cargo-deny-action v2.

Re-check before bumping.

## ADR-0011 — MCP server: rmcp 3.10 over stdio, panic guard per handler
**Date:** 2026-11-17 · **Status:** accepted

- **SDK:** `rmcp` 3.10, the official Rust SDK, with features `server`, `macros` and `transport-io`. Its default handler accepts every known protocol version (2024-11-05 … 2026-07-28), so older harnesses still negotiate.
- **Server lifetime:** `ehai mcp` serves stdio on a current-thread tokio runtime and exits cleanly when the client closes stdin. There is no daemon.
- **Panic guard:** every handler body runs through `ehai_mcp::guard`, which uses `tokio::spawn` and converts `JoinError::is_panic` into a structured error. One bad request can't kill the session.
- **Prefs resource URI:** `ehai://prefs` is fixed. The brand renames the MCP *server*, never the URIs, because internal identifiers stay `ehai` (spec §2.4).
- **Verified:** real Claude Code 2.1.179 shows `plugin:ehai:ehai … ✔ Connected` in `claude mcp list`.

## ADR-0012 — Claude Code adapter: a local marketplace with one plugin that bundles the MCP server
**Date:** 2026-11-17 · **Status:** accepted
**Sources:** code.claude.com/docs/en/plugins-reference, /plugin-marketplaces and /skills, fetched 2026-11-17. Checked against Claude Code CLI 2.1.179.

- **Plugin layout:**
  - `ehai` generates `~/.ehai/adapters/claude-code/.claude-plugin/marketplace.json`.
  - The marketplace (`<brand>-local`) lists a single plugin (`<brand>`).
  - The plugin holds `.mcp.json` (pointing at `<abs exe> mcp`) and `skills/<name>/SKILL.md`. Skills replace legacy `commands/`, as the docs recommend.
- **Install path:** `claude plugin marketplace add <dir>`, then `claude plugin install <brand>@<brand>-local --scope user`.
  - We chose this over `claude mcp add --scope user`: one install registers both the MCP server and the skills, and one `marketplace remove` reverses both.
  - It's idempotent: each step runs only if `--json` listings show it's missing.
- **Plugin versioning:** the docs suggest local-path plugins load in place, but 2.1.179 copies them into a **versioned cache**, and `plugin update` is a no-op when the version is unchanged. So `plugin.json` `version` = `<ehai version>+<FNV-1a hash of generated content>`. Changed content gets a new version, and install then runs `marketplace update` + `plugin update`. This was verified live: changing the role in `init` refreshed the installed skill.
- **Brand rename:** marketplaces whose path is our root but whose name differs are removed before the new one is added.
- **Reserved names:** `Brand::validate` rejects names Claude Code refuses for plugins (`claude`, `claude-*`, `anthropic-*`, `cc-plugin-*`).
- **Test strategy:**
  - Unit tests use a fake `claude` shell script, injected via `EHAI_CLAUDE_BIN`.
  - The live check used `CLAUDE_CONFIG_DIR=<tmp>` so the real user config was never touched.
  - `claude plugin validate` passes on both manifests.

## ADR-0013 — `ehai init` scope and behaviour
**Date:** 2026-11-17 · **Status:** accepted

- **Questions:** role, harnesses (pre-checked), compliance, sandbox, and repo-profile confirmation. That's ≤ 5, tested.
- **Tracker question:** moved to **M2**, where it can list the user's GitHub Projects live. Asking for the tracker kind alone now would be a wasted question.
- **No half-applied state:** nothing is written until all answers are in, so cancelling leaves nothing behind. Apply steps are idempotent, so rerunning `init` resumes.
- **Esc vs Ctrl-C:** cliclack reports both as the same `Interrupted`. A cancel opens "Go back / Quit": Esc then Enter goes back, and a second cancel quits with exit 130 and "Nothing was changed".
- **Retry:** a failed apply step offers "Retry?" defaulting to **no**, so `--yes` and non-TTY runs never loop.
- **Flags:** every question has one (`--role --harness --compliance --sandbox --no-repo`), and a flag skips its question.
- **Re-running:** existing config values become the defaults.
- **Chosen harnesses** are stored in `adapters.enabled`. `doctor` verifies, and `adapters regenerate` refreshes, only those.

## ADR-0014 — When repo onboarding runs
**Date:** 2026-11-17 · **Status:** accepted

- **Spec:** says onboarding runs "the first time ehai runs in a new repo (via any command)".
- **M1:** it runs from `ehai init` and `ehai repo init`.
  - `ehai mcp` must never prompt, because stdout is the protocol channel.
  - `doctor`, `config` and `adapters` are non-interactive.
  - The auto-trigger will hook into the interactive commands as they arrive (M3+).
- **What it writes:** `<repo>/.ehai/profile.toml` `[detected]` is rewritten on each scan. Other tables (team decisions) are preserved.
- **Local state:** a `.ehai/.gitignore` keeps it out of git without editing the repo's own `.gitignore`.
- **Secrets:** env files are recorded by **name only**, never read.

## ADR-0015 — External commands always have a timeout
**Date:** 2026-11-17 · **Status:** accepted

`ehai_core::process::output` spawns, drains stdout and stderr on threads (so a full pipe can't deadlock), and kills the process after a timeout. It covers `gh`, `git --version` and `claude` calls (120 s). Retries with backoff will be added for network calls in M2.

## ADR-0016 — `self install`, `connect`/`disconnect`, one-command `uninstall`
**Date:** 2026-11-17 · **Status:** accepted

- **`ehai self install`:**
  - Copies the running binary to `~/.ehai/bin/ehai` (copy-then-rename, so it's safe while running).
  - Adds the brand alias (`EHAI_NAME`/`brand.name`; a symlink on Unix).
  - Writes a marked `# >>> ehai >>>` PATH block into the login shell's rc: bash → `.bashrc`, zsh → `.zshrc`, fish → `conf.d/ehai.fish`, otherwise `.profile`. A `.ehai-backup` is taken before the first edit.
  - Re-points enabled adapters at the installed binary.
  - The M8 one-line installer downloads a release and calls this.
  - Windows: PATH is set by `install.ps1` (M8); `self install` prints the instruction.
- **Adapter binary path:** `adapter_ctx` prefers `~/.ehai/bin/ehai` over `current_exe()`. Harnesses then survive `cargo clean` or a deleted download folder. This was the bug where M1's `init` pointed Claude Code at `target/debug/ehai`.
- **`ehai connect <id>…` / `ehai disconnect <id>… | --all`:** friendly aliases for `adapters install/remove`. They also update `adapters.enabled`.
- **`ehai uninstall [--yes]`:**
  - Asks first, defaulting to *no*. A non-TTY run without `--yes` does nothing.
  - Unregisters **every** adapter, not only the enabled ones.
  - Strips the PATH blocks from all candidate rc files.
  - Deletes only entries ehai owns inside `~/.ehai`, and refuses if `EHAI_HOME` is the user's home or has no parent. Unknown files are kept and reported.
  - Skips file logging, so it doesn't recreate `~/.ehai/logs`.
  - Repo `.ehai/` profiles are left alone: they belong to the repo and may be committed.
- **Testing:** the rc-file location is passed in explicitly (`remove_files(home, user_home)`), so tests can never touch the developer's real dotfiles. A sandboxed-`HOME` lifecycle test proves that install → connect → disconnect → uninstall leaves no traces and restores `.bashrc` byte-for-byte.
