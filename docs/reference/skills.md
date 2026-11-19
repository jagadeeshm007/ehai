# Skills Reference

Skills are the user-facing layer over ehai's MCP tools. Each skill is a slash command (`/ehai:<name>`) that handles the multi-turn question loop, calls the right tools in order, and formats output for your role.

Skills are installed per-harness by `ehai connect <harness>` or `ehai init`. They are plain text templates that instruct the AI how to call the MCP tools.

---

## PM Skills

### `/ehai:plan`

**When to use:** When you have an idea, a feature request, or a problem statement and need to produce a full PRD.

**What it does:**

1. Asks 3–5 clarifying questions using multiple-choice where possible (problem, users, goals, scope, success metrics)
2. Produces a PRD with Mermaid user flows, risks, out-of-scope list
3. Calls `plan_explore` in a loop (up to 3 rounds) to surface gaps — missing metrics, missing edge cases, no out-of-scope list
4. Asks for your approval before `save_prd` writes anything

**Usage:**

```
/ehai:plan [optional initial description]
```

If you provide an initial description, ehai skips the first question and starts with that as the problem statement.

**Output:** PRD document (Markdown + Mermaid). In Claude Desktop it writes a Claude Doc. In CLI harnesses it writes `docs/prd/<slug>.md` in your repo.

**MCP tools called:** `plan_explore`, `save_prd`

---

### `/ehai:breakdown`

**When to use:** After a PRD is saved. Turns the PRD into work items.

**What it does:**

1. Reads the saved PRD (calls `get_plan`)
2. Generates epics → stories → tasks with Given/When/Then acceptance criteria
3. Assigns priority (P0–P3), size (XS–XL), and platform tags (web, mobile, api)
4. Shows a board preview and waits for your approval
5. Calls `save_breakdown` to persist

**Usage:**

```
/ehai:breakdown [prd slug, optional — uses latest if omitted]
```

**MCP tools called:** `get_plan`, `save_breakdown`

---

### `/ehai:sync`

**When to use:** After a breakdown is saved and you're ready to push to your tracker.

**What it does:**

1. First runs a dry-run (`apply: false`) to show what would change: N items to create, M items to update
2. Waits for confirmation
3. Pushes with `apply: true` — idempotent: re-running never duplicates items

**Usage:**

```
/ehai:sync [prd slug, optional]
```

**Idempotency:** Each synced item gets an `ehai-id` written to its tracker body footer. Subsequent runs diff local vs remote by that ID, never creating duplicates.

**MCP tools called:** `sync_plan`

---

### `/ehai:status`

**When to use:** At any point to get a progress snapshot across all epics.

**What it does:**

1. Calls `status_report` (fetches from tracker)
2. Formats: % done per epic, stale items, open PRs, risk flags
3. Outputs Markdown + Mermaid progress diagram
4. Optionally writes an HTML report to `~/.ehai/reports/`

**Usage:**

```
/ehai:status [--html]
```

**MCP tools called:** `status_report`

---

### `/ehai:triage`

**When to use:** When a bug is reported, a user request comes in, or an issue needs to be turned into a tracker item.

**What it does:**

1. If the report is vague, asks at most 3 questions: what happened vs expected, how often, where
2. Classifies type (Bug/Story/Task), priority (P0–P3), size (XS–XL)
3. Checks for duplicates against existing items
4. If a possible duplicate is found, asks: same issue or new?
5. Creates the tracker item

**Priority guide:**
- **P0** — Critical impact, no workaround
- **P1** — Major impact, workaround exists
- **P2** — Minor impact
- **P3** — Cosmetic

**Usage:**

```
/ehai:triage [description]
```

**MCP tools called:** `triage`

---

### `/ehai:assign`

**When to use:** To assign, reassign, or take a task.

**Usage:**

```
/ehai:assign  #42 to priya
/ehai:assign  "notification toggle" to me
/ehai:assign  #42 remove jagadeesh
```

**MCP tools called:** `assign_task`, `team`

---

### `/ehai:prefs`

**When to use:** To view or edit your role and learned preferences.

**What it shows:**
- Your current role (pm / dev / both)
- High-confidence learned preferences (planning style, POC taste, estimate scale)
- Recent observations

**Usage:**

```
/ehai:prefs
/ehai:prefs set role dev
/ehai:prefs delete planning.granularity
```

**MCP tools called:** `ehai://prefs` resource

---

## Developer Skills (planned)

!!! note "Not built yet"
    These four skills are planned (spec §5.3) and are **not** installed today. Developers currently pick up work with [`/ehai:assign`](#ehaiassign) and the board. The descriptions below are the intended behaviour.

### `/ehai:next`

**When to use:** When you want to know what to work on next.

**What it does:** Suggests the next unblocked task by priority, assignee, and dependency order.

**Usage:**

```
/ehai:next
```

**MCP tools called:** `status_report`

---

### `/ehai:start`

**When to use:** When picking up a task. Creates the branch and loads context.

**What it does:**

1. Looks up the item in the tracker (by number, URL, or partial title)
2. Creates `feat/<id>-<slug>` branch from main
3. Loads the task's full context (acceptance criteria, description, linked PRD)
4. Sets the card to In Progress

**Usage:**

```
/ehai:start #42
/ehai:start notification toggle
```

**MCP tools called:** `assign_task` (sets to In Progress)

---

### `/ehai:pr`

**When to use:** When you're ready to open a pull request.

**What it does:**

1. Reads the current branch name and infers the linked task number
2. Generates a PR description that:
   - Links the issue (`Fixes #42`)
   - Maps each acceptance criterion to a checkbox
   - Flags any unverified criteria
3. Shows the draft and waits for approval
4. After PR is opened, moves the card to In Review

**Usage:**

```
/ehai:pr
```

**MCP tools called:** `comment_task` (after PR created externally)

---

### `/ehai:done`

**When to use:** When a PR is merged and the task is complete.

**What it does:** Verifies acceptance criteria are satisfied (asks if any are unchecked) and moves the card to Done.

**Usage:**

```
/ehai:done [#id, defaults to current branch task]
```

**MCP tools called:** `assign_task` (sets to Done)

---

## POC commands

POCs are driven from the `ehai poc` CLI, and the same operations are exposed as `poc_*` MCP tools your harness can call. There are **no `/ehai:poc` slash commands**. `<feature>` is a short slug (e.g. `notifications`); `<n>` is the POC number.

### `ehai poc new`

**When to use:** When you want to prototype a UI feature without touching main.

**What it does:**

1. Extracts design tokens from your repo (Tailwind, NativeWind, MUI themes, etc.)
2. Creates an isolated git worktree at `~/.ehai/worktrees/<repo>/<feature>/poc-<n>` on branch `ehai/poc/<feature>/poc-<n>`
3. Provisions the worktree: copies `.env*`, installs dependencies, allocates a unique port
4. Registers the POC in the manifest and prints the worktree, port, URL and design context

```bash
ehai poc new notifications "notification settings screen"
```

**MCP tool:** `poc_new`

---

### `ehai poc iterate`

**When to use:** To refine an existing POC with feedback.

```bash
ehai poc iterate notifications 1 "Make the section headers sticky and add an All Off button"
```

Creates the next POC (e.g. `poc-2`) with lineage pointing to `poc-1`. Both POCs remain accessible.

**MCP tool:** `poc_iterate`

---

### `ehai poc compose`

**When to use:** To synthesize a new POC that takes the best parts of multiple earlier ones.

**What it does:** Starts from the base branch; the AI reads the referenced POCs' diffs and resynthesizes the described parts — not a blind cherry-pick. Lineage records each parent.

```bash
ehai poc compose notifications "Take the sticky headers from poc-1 and the toggle design from poc-2" --from 1 --from 2
```

**MCP tool:** `poc_compose`

---

### `ehai poc list`

```bash
ehai poc list
ehai poc list notifications      # filter by feature
```

---

### `ehai poc compare`

```bash
ehai poc compare notifications 1 2
```

Writes a side-by-side HTML comparison report and opens it in the browser.

---

### `ehai poc open`

```bash
ehai poc open notifications 2
```

Prints the worktree path, dev-server command and URL for the POC.

---

### `ehai poc run` / `ehai poc shell`

**When to use:** to start a POC or run any command in it without `cd`-ing into its worktree. Works from anywhere inside the repo.

```bash
ehai poc run checkout 2                 # start the detected dev server
ehai poc run checkout 2 -- pnpm test    # any command, inside the worktree
ehai poc shell checkout 2               # your shell, inside the worktree; `exit` to come back
```

Commands run in `~/.ehai/worktrees/<repo>/<feature>/poc-<n>` with that worktree's `.env.ehai.local` loaded, so each POC uses its own ports (`PORT`, `COMPOSE_PROJECT_NAME`, …). The exit code is passed through, so `ehai poc run … -- pnpm test` works in scripts. `ehai poc shell` also sets `EHAI_POC=<feature>/poc-<n>`, which you can show in your prompt.

---

### `ehai poc promote`

**When to use:** When you've chosen a POC and want to open a PR to merge it.

**Safety:** Always asks for confirmation (`--yes` does not skip it). The POC branch is never merged silently.

```bash
ehai poc promote notifications 2
```

---

### `ehai poc discard`

```bash
ehai poc discard notifications 1
```

Archives the POC as branch `ehai-archive/<feature>/poc-1` and removes the worktree. Nothing is hard-deleted.

---

## Utility

### `/ehai:help`

Lists all available skills with one-line descriptions.

```
/ehai:help
```

---

## Customizing skills

Skills are plain text files. After connecting a harness, they live in the harness's prompt/command directory. To customize:

1. Find the file: `ehai adapters list` shows the installed paths.
2. Edit the template outside the `# >>> ehai >>>` ... `# <<< ehai <<<` markers.
3. Changes inside the markers are overwritten by `ehai adapters regenerate`.

To rename all slash commands to a custom prefix:

```toml
# ~/.ehai/config.toml
[brand]
name = "acme"
```

```bash
ehai adapters regenerate    # all skills become /acme:plan, /acme:start, etc.
```
