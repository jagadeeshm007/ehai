# Setup & Connections

This guide walks through every connection ehai needs: GitHub auth, harness wiring, and optional services.

---

## GitHub

ehai needs a GitHub token with `project` scope to manage GitHub Projects v2 — creating fields, syncing items, linking PRs, and reading team members.

### Option A: gh CLI

```bash
# Install gh if you don't have it
# macOS:  brew install gh
# Linux:  https://cli.github.com/manual/installation
# Windows: winget install gh

gh auth login                     # first-time login (browser flow)
gh auth refresh -s project        # add project scope to an existing session
```

ehai reads the token via `gh auth token` automatically. No config needed.

### Option B: env var

```bash
export GITHUB_TOKEN=ghp_xxxxxxxxxxxx   # or GH_TOKEN — either works
```

Add to your shell's RC file to persist it. ehai picks up either variable.

### Option C: no GitHub account

At `ehai init`, choose **local tracker** when asked. All PM flows, PRD storage, breakdown, and status work fully offline. Add GitHub later:

```bash
ehai tracker set github    # prompts for auth if needed
```

### Verify

```bash
ehai doctor
# ✔ GitHub auth (jagadeeshm007, project scope)
# ✔ tracker reachable (My Project · 23 items)
```

If you see a scope error:

```bash
gh auth refresh -s project
```

---

## Harnesses

Connect ehai to any AI coding harness. `ehai init` wires everything automatically — use the commands below to connect or reconnect a specific harness.

### Claude Code

Claude Code connects via MCP over stdio. After `ehai init` this is wired automatically as a user-scope MCP server.

```bash
ehai connect claude-code

# Or directly with the claude CLI:
claude mcp add --scope user ehai -- ehai mcp
```

**Verify** — open Claude Code and run `/ehai:help`. It calls the `ping` tool, says whether ehai is connected, and lists the commands.

**Troubleshoot:**

```bash
ehai doctor                       # checks adapter status
ehai adapters list                # shows all adapters + detect result
ehai disconnect claude-code       # clean disconnect
ehai connect claude-code          # reconnect
```

---

### Claude Desktop

Claude Desktop uses skills (`.zip`) + MCP connector.

```bash
ehai connect claude-desktop
```

This writes the MCP server entry to your `claude_desktop_config.json` and generates a skills ZIP at `~/.ehai/adapters/claude-desktop/ehai-skills.zip`.

Then in Claude Desktop: **Settings → Skills → Upload ZIP**.

!!! note "PM on Claude Desktop"
    PMs get the full planning flow — PRD generation, breakdown, sync — without touching a terminal. The skills ZIP includes PM-optimised prompts with visual-first output (Mermaid diagrams, tables).

---

### Codex CLI

```bash
ehai connect codex
```

Writes to `~/.codex/config.toml` under `[mcp_servers]` and places prompt files in `~/.codex/prompts/`.

---

### Gemini CLI

```bash
ehai connect gemini
```

Writes to `~/.gemini/settings.json` under `mcpServers` and places TOML command files in `~/.gemini/commands/`.

---

### Cursor

```bash
ehai connect cursor
```

Writes to `~/.cursor/mcp.json` and places rule files in `~/.cursor/rules/`.

---

### VS Code / GitHub Copilot

```bash
ehai connect vscode
```

Writes to VS Code's user `mcp.json` (under `servers` key) and places prompt files in the VS Code prompts directory.

---

### OpenCode

```bash
ehai connect opencode
```

Writes to `~/.config/opencode/config.json` and places prompt files in the opencode commands directory.

---

### Windsurf

```bash
ehai connect windsurf
```

Writes to `~/.codeium/windsurf/mcp_server_config.json` and places workflow files.

---

### Any other harness

```bash
ehai connect agents-md
```

Writes a reference card to `~/AGENTS.md` describing ehai's MCP server config and all available slash commands. Any harness that reads `AGENTS.md` picks this up automatically.

---

### OpenAI-compatible APIs

ehai's engine makes no LLM calls itself — all reasoning happens inside your harness. If your harness connects to an OpenAI-compatible endpoint (Azure OpenAI, Ollama, LM Studio, Groq, etc.):

1. Configure your harness's model/API endpoint as usual.
2. Run `ehai connect <harness>` to wire up MCP.
3. Use `/ehai:plan`, `/ehai:sync`, etc. — the harness calls ehai's MCP tools the same way regardless of model.

---

## Managing connections

```bash
ehai adapters list                 # show all adapters: detected / installed / status
ehai adapters regenerate           # re-render all installed adapters (after rename or update)
ehai disconnect cursor             # remove one adapter
ehai disconnect --all              # remove all adapters
ehai adapters install vscode       # install a specific adapter
ehai adapters remove opencode      # remove a specific adapter
```

---

## First-time repo setup

The first time any ehai command runs in a repo, it detects:

- Stack (React Native, Vite, Next.js, Go, Python, pnpm monorepo, etc.)
- Design tokens (Tailwind config, CSS variables, MUI/NativeWind themes)
- Conventions (naming, test framework, commit style, branch naming)
- Existing tracker fields and label sets

It shows a one-screen summary and asks to confirm, then writes `.ehai/profile.toml`. Rerun anytime:

```bash
ehai repo init
```

---

## Removing ehai

### Disconnect one harness

Remove ehai from a single AI harness without touching anything else:

```bash
ehai disconnect claude-code     # unregisters MCP, removes generated files
ehai disconnect cursor
ehai disconnect --all           # removes every connected harness at once
```

ehai only removes the files it generated (inside its own `# >>> ehai >>>` block or dedicated config key). Your own edits to those files are preserved.

---

### Remove ehai from a repo

ehai writes one directory per repo: `.ehai/` at the repo root. It contains the detected stack profile, local plan cache, and optional POC manifest. It is never required for the binary to run.

```bash
# From inside the repo:
rm -rf .ehai/
```

If you committed `.ehai/profile.toml` to share conventions with the team, also remove it from git:

```bash
git rm -r .ehai/
git commit -m "chore: remove ehai repo profile"
```

Nothing else in the repo is touched by ehai (no changes to `package.json`, `go.mod`, etc.).

---

### Full uninstall

Remove ehai completely from your machine — binary, config, prefs, adapters, logs, cached data, and PATH entries:

```bash
ehai uninstall
```

This asks for confirmation, then:

- Disconnects every harness (`ehai disconnect --all`)
- Deletes `~/.ehai/` contents: `config.toml`, `prefs/`, `adapters/`, `bin/`, `logs/`, `audit/`, `cache/`, `worktrees/`, `ports.json`
- Removes the `export PATH` line (or fish equivalent) from your shell RC files
- Removes the `ehai` link the installer put in a folder on your PATH (`~/.local/bin`, `/usr/local/bin`, …), only if it points into `~/.ehai`
- If you saved a token at `~/.ehai/token`, that file is deleted too

Repo profiles (`.ehai/` inside your repos) are **not** deleted — remove those manually per repo if needed.

To skip the confirmation prompt (CI or scripting):

```bash
ehai uninstall --yes
```

---

## Device migration

Move your ehai setup to a new machine:

```bash
# Old machine
ehai migration export                     # → ehai-backup-20261118.zip

# New machine (after installing ehai)
ehai migration import ehai-backup-*.zip   # restores config, prefs, local tracker
ehai adapters regenerate                  # re-wires harnesses with new paths
```

The ZIP contains your config, preferences, and local tracker data. Harness adapter files are excluded because they contain machine-specific absolute paths — `adapters regenerate` rebuilds them correctly.
