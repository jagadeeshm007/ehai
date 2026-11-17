# Contributing

## Setup
```bash
git config core.hooksPath .githooks   # conventional-commit check
cargo install --locked cargo-nextest cargo-deny cargo-insta
```

## Before every commit
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo nextest run
cargo deny check
```
- To review snapshot changes, run `cargo insta review`.
- Tests must never touch your real `~/.ehai`, so set `EHAI_HOME` to a tempdir.
- Never commit secrets, `.env` files, or real patient data. That applies to fixtures too.

## Commit messages
Use [Conventional Commits](https://www.conventionalcommits.org/): `type(scope)!: summary`.
- Types: `feat fix docs style refactor perf test build ci chore revert`.
- Example: `feat(tracker): add GitHub Projects adapter`.

Release notes are generated from these messages by git-cliff (`cliff.toml`).

## Cutting a release
```bash
just release 0.1.0-alpha.2   # bumps the version, regenerates CHANGELOG.md, commits, tags
git push && git push --tags  # triggers .github/workflows/release.yml
```
Needs `git-cliff` on PATH. Modifying `install/install.sh`? Lint it first: `shellcheck install/install.sh` (also runs in CI).

## Docs
- Update `docs/PROGRESS.md` at the end of each milestone.
- Add an entry to `docs/DECISIONS.md` for every non-obvious choice.
- Doc-only changes don't run CI (ADR-0005).
