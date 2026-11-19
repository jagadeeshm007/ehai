# Quickstart

Zero to your first `/ehai:plan` in about 5 minutes.

## Requirements

| Needed | For | Notes |
|---|---|---|
| macOS 12+, Windows 10 1809+, or Linux 5.13+ (x86_64 or arm64; Windows x86_64) | everything | One static binary, no runtime to install |
| `curl`, `tar` and `sha256sum` or `shasum` | installer | Already on macOS and almost every Linux. Windows uses built-in PowerShell |
| `git` 2.20+ | POCs, worktrees, `/ehai:start` | `ehai doctor` checks it |
| `gh` **or** a `GITHUB_TOKEN` | GitHub Projects tracker | ehai reuses `gh auth token`; without `gh`, set `GITHUB_TOKEN` (or `GH_TOKEN`). Not needed for the local tracker |
| Chrome / Chromium | optional | POC screenshots only; skipped if missing |
| Your project's own tools (`pnpm`/`npm`/`yarn`/`bun`, `go`, `uv`/`poetry`/`pip`) | optional | Only to install dependencies and run previews inside POC worktrees |

Not needed: Node, Python, Docker or `minisign` (the installer gets a temporary copy to check the signature).

## 1. Install

```bash
curl -fsSL https://raw.githubusercontent.com/jagadeeshm007/ehai/main/install/install.sh | sh
```

This downloads the right binary for your OS and architecture, verifies the checksum and minisign signature, installs to `~/.ehai/bin`, and adds it to your PATH.

You don't need minisign installed. If it's missing, the installer downloads a temporary copy of the official build (pinned by SHA-256), uses it to check the signature, and deletes it. If that download fails, it warns and still checks the SHA-256 checksum. Set `EHAI_REQUIRE_SIGNATURE=1` to stop the install instead.

## 2. Init

```bash
ehai init
```

The wizard runs automatic detection (spinner per check), shows a summary card of what it found, then asks at most 5 questions:

| Question | Default |
|---|---|
| Your role | PM + Dev |
| Harnesses to wire up | auto-detected, pre-checked |
| Tracker | GitHub Projects (or local if no `gh`) |
| Data policy mode | warn |
| Sandbox mode | restricted |

It applies each step with a live checklist and shows the three most useful commands for your role at the end.

!!! tip "No GitHub account?"
    Choose **local tracker** at the tracker question. All PM flows work offline. Add GitHub later with `ehai tracker set github`.

## 3. Open your project

```bash
cd my-app
```

The first time ehai runs in a repo it detects the stack (React Native, Vite, Next.js, Go, Python, etc.), reads design tokens, and writes `.ehai/profile.toml`. You'll see a one-screen summary and a confirm prompt.

## 4. Plan something

In Claude Code (or any connected harness):

```
/ehai:plan A settings screen where users can toggle notification preferences
```

ehai interviews you with multiple-choice questions, then produces a full PRD — problem statement, user flows (Mermaid), success metrics, risks — and runs a gap-finder loop to deepen it before handing off to breakdown.

## 5. Break it down and sync

```
/ehai:breakdown
/ehai:sync
```

`breakdown` turns the PRD into epics → stories → tasks with Given/When/Then acceptance criteria. It shows a board preview and waits for your approval before syncing. `sync` pushes to your tracker idempotently — running it twice never duplicates items.

## 6. Start a task

```
/ehai:start #12
```

Creates `feat/12-notifications-toggle`, loads the task's acceptance criteria into the session, and sets the card to In Progress.

---

## Common next steps

- `ehai doctor` — check health and get fix hints
- `ehai update` — update to the latest release on your channel
- `/ehai:poc new "notifications toggle screen"` — build an isolated UI prototype
- [Data policy guide →](compliance.md)
- [Full config reference →](../reference/config.md)
