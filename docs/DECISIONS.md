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
