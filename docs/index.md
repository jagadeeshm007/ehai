# ehai

**AI-SDLC ops engine for any team.**

PMs plan requirements and push them to GitHub Projects. Developers carry tasks through the dev lifecycle from any AI coding harness. Anyone generates isolated UI/UX proof-of-concepts that match the app's design system.

---

## Install

=== "macOS / Linux"

    ```bash
    curl -fsSL https://raw.githubusercontent.com/jagadeeshm007/ehai/main/install/install.sh | sh
    ```

=== "Windows (PowerShell)"

    ```powershell
    irm https://raw.githubusercontent.com/jagadeeshm007/ehai/main/install/install.ps1 | iex
    ```

=== "From source"

    ```bash
    cargo build --release
    ./target/release/ehai self install
    ```

Then run `ehai init` — the wizard auto-detects your harnesses, connects GitHub, and wires everything up in under 2 minutes.

---

## What you get

<div class="grid cards" markdown>

-   **PM flow**

    Interview → PRD → epics → stories → tasks, synced to GitHub Projects
    with one command. Works in Claude Desktop and any AI terminal harness.

-   **Developer flow**

    Tasks with acceptance criteria on the board, assigned with `/ehai:assign`
    from any AI coding tool. Planned: `/ehai:next`, `/ehai:start`, `/ehai:pr`,
    `/ehai:done` to automate branches, PR text and card moves.

-   **POC engine**

    Isolated worktrees with auto-provisioned `.env`, dependencies, and ports.
    Design tokens extracted from your real app. Screenshots, lineage,
    compose, promote — all without touching `main`.

-   **Data policy guard**

    Pluggable domain profiles (tech, health, finance, GDPR) check every
    tracker write, PRD, and mock data fixture before it lands anywhere.

</div>

---

## Supported harnesses

| Harness | Commands |
|---|---|
| Claude Code | `/ehai:` slash commands + MCP tools |
| Claude Desktop | Skills ZIP + MCP connector |
| Codex CLI | prompt files |
| Gemini CLI | TOML commands |
| Cursor | rule files + MCP |
| VS Code / Copilot | prompt files + MCP |
| OpenCode | prompt files |
| Windsurf | workflows |

---

## Next steps

- [How ehai works](guide/how-it-works.md) — the problem, philosophy, architecture and flow diagrams
- [With spec-driven tools](guide/spec-driven.md) — build with AWS AI-DLC, GitHub Spec Kit or Kiro; track, maintain and improve with ehai

[Quickstart →](guide/quickstart.md){ .md-button .md-button--primary }
[Config reference →](reference/config.md){ .md-button }
