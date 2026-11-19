# Contributing to ehai

Thank you for contributing. ehai is MIT-licensed and welcomes bug fixes, documentation improvements, new harness adapters, tracker adapters, and feature work aligned with the roadmap.

---

## Table of contents

1. [Getting started](#getting-started)
2. [Project layout](#project-layout)
3. [Development workflow](#development-workflow)
4. [Coding standards](#coding-standards)
5. [Testing requirements](#testing-requirements)
6. [Commit conventions](#commit-conventions)
7. [Branch naming](#branch-naming)
8. [Pull request process](#pull-request-process)
9. [Good first issues](#good-first-issues)
10. [Reporting bugs](#reporting-bugs)

---

## Getting started

**Prerequisites**

- Rust stable (1.78+). Install via [rustup](https://rustup.rs).
- `cargo-nextest` — `cargo install cargo-nextest`
- `cargo-insta` — `cargo install cargo-insta` (for snapshot tests)
- `cargo-deny` — `cargo install cargo-deny` (run before submitting a PR)
- `git` 2.35+ (for worktree operations in tests)

**Clone and build**

```bash
git clone https://github.com/jagadeeshm007/ehai
cd ehai
cargo build --workspace
just setup-hooks   # wire up pre-commit / pre-push / commit-msg checks (once per clone)
```

**Run tests**

```bash
cargo nextest run --workspace
```

**Lint (or use `just check` to run all three at once)**

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo nextest run --workspace
```

All three must be clean before submitting a PR. The git hooks run them automatically:
- `pre-commit` — fmt + clippy on every commit (fast, <5s)
- `pre-push` — fmt + clippy + full test suite before every push
- `commit-msg` — enforces conventional commit format

Skip tests on a push when you need to: `SKIP_TESTS=1 git push`

---

## Project layout

```
ehai/
├── crates/
│   ├── ehai-core/      # config, versioned schema + migrations, brand, errors
│   ├── ehai-tracker/   # Tracker trait + adapters (github, local)
│   ├── ehai-planner/   # PRD model, breakdown, acceptance criteria, triage
│   ├── ehai-poc/       # POC lifecycle: worktrees, manifest, lineage, compose
│   ├── ehai-env/       # worktree env provisioning (env files, ports, deps)
│   ├── ehai-stack/     # stack detection, design-token extraction, dev-server runners
│   ├── ehai-prefs/     # preference learning store
│   ├── ehai-guard/     # data policy guard (domain profiles + custom patterns)
│   ├── ehai-adapters/  # harness installers/generators
│   ├── ehai-update/    # self-update, rollback, health check
│   ├── ehai-mcp/       # MCP server: tools, resources, prompts
│   └── ehai-cli/       # binary entry point: CLI commands
├── templates/          # slash-command and skill templates (embedded via include_str!)
├── install/            # install.sh, install.ps1
├── docs/               # MkDocs Material site
└── tests/e2e/          # end-to-end test suite
```

**Key invariants**

- All logic lives in crates. The `ehai-cli` binary is a thin shell.
- `ehai-mcp` owns the MCP API surface. Tool parameter structs are schema-exported via `JsonSchema`.
- Templates live in `templates/` and are embedded at compile time. Never generate adapter files in tests without going through the renderer.
- The `Tracker` trait in `ehai-tracker` is the only way to touch a tracker. No direct GitHub API calls outside that crate.

---

## Development workflow

1. Fork the repo and clone your fork. Run `just setup-hooks` once.
2. Create a branch (see [Branch naming](#branch-naming)).
3. Make your change, run tests, fix lints.
4. If you changed template text, run `INSTA_UPDATE=always cargo nextest run --workspace` to regenerate snapshots, then review them with `cargo insta review`.
5. **Update docs before committing** (see table below).
6. Open a pull request against `main`.

**Docs update checklist** — apply whichever rows match your change:

| Changed | Update |
|---|---|
| New or changed CLI command / MCP tool | `README.md` reference table + `docs/reference/` |
| New or changed config key | `docs/reference/config.md` + `README.md` config section |
| New or changed user-facing behavior | `docs/guide/` (quickstart, use-cases, or setup) |
| Non-obvious architectural decision | `docs/DECISIONS.md` (new ADR entry) |
| Milestone completed | `docs/PROGRESS.md` |
| Version bump | `CHANGELOG.md` via `just release <version>` — never hand-edit |

---

## Coding standards

- **`thiserror` in libraries, `anyhow` in the binary.** Never use `unwrap()` on paths that can fail in production; use `?` and propagate.
- **No `unsafe`.** There is no `unsafe` in the codebase today. New `unsafe` requires a detailed justification comment and a reviewer sign-off.
- **No hardcoded secrets.** Ever. Not in tests, not in fixtures.
- **Error messages are user-facing.** Write them as full sentences with a fix hint: `"GitHub token missing project scope — run: gh auth refresh -s project"`.
- **Prefer existing dependencies.** Check `Cargo.toml` before adding a new one. Each new dependency requires a note in `docs/DECISIONS.md` explaining why the existing stack couldn't cover it.
- **Comments only for WHY.** Code explains what; comments explain why (hidden constraint, non-obvious invariant, workaround for a specific upstream bug).
- **Formatting.** Run `cargo fmt` before committing. The repo uses the default rustfmt settings.
- **Clippy is enforced.** `cargo clippy --all-targets -- -D warnings` must be clean. No `#[allow(clippy::...)]` without a comment explaining why.

---

## Testing requirements

| Area | Requirement |
|---|---|
| Core, guard, tracker, planner, prefs, update | ≥80% line coverage |
| Snapshot tests (adapter templates) | Every generated file per harness × brand name |
| Property tests | Config migrations; PII detector; port allocator |
| MCP protocol | All tools callable over stdio; malformed input never crashes the server |
| Git invariants | Source checkout unchanged byte-for-byte after every POC operation |

**Regenerate snapshots**

```bash
INSTA_UPDATE=always cargo nextest run --workspace
cargo insta review          # accept or reject each changed snapshot
```

---

## Commit conventions

ehai uses [Conventional Commits](https://www.conventionalcommits.org/). `git cliff` reads commit messages to generate CHANGELOG entries.

```
<type>(<scope>): <short summary>
```

| Type | When to use |
|---|---|
| `feat` | New feature or capability |
| `fix` | Bug fix |
| `perf` | Performance improvement |
| `refactor` | Restructuring without behaviour change |
| `test` | Adding or fixing tests |
| `docs` | Documentation only |
| `build` / `ci` | Build system or CI |
| `chore` | Dependency bumps, release commits |

**Breaking changes** — add `!` after the type:

```
feat(mcp)!: rename `phi` parameter to `policy`

BREAKING CHANGE: ...
```

A commit-msg hook (`git config core.hooksPath .githooks`) enforces the format locally.

---

## Branch naming

```
feat/<short-description>
fix/<short-description>
docs/<short-description>
refactor/<short-description>
test/<short-description>
chore/<short-description>
```

---

## Pull request process

1. **Title** — follow the commit convention: `feat(tracker): Linear adapter`.
2. **Description** — use the PR template. Link the issue (`Fixes #123`). Add a test plan.
3. **CI must be green.** Tests pass, clippy clean, fmt clean, cargo-deny clean.
4. **Snapshots** — regenerated `.snap` files go in the same PR.
5. **One concern per PR.**
6. **Review turnaround** — maintainers aim to review within 48 hours.

---

## Good first issues

Look for [`good first issue`](https://github.com/jagadeeshm007/ehai/labels/good%20first%20issue) on GitHub:

- Writing a missing test for an existing code path
- Improving an error message with a fix hint
- Adding a new compliance pattern (with property tests)
- Fixing a documentation typo
- Adding a snapshot test for a harness template

---

## Reporting bugs

Use the [Bug Report template](https://github.com/jagadeeshm007/ehai/issues/new?template=bug_report.md). Include:

- `ehai --version` output
- OS and architecture
- The exact command and full output
- What you expected

For security vulnerabilities, **do not** open a public issue — contact the maintainer directly.
