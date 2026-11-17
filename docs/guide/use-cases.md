# Use Cases

Real workflows with ehai, organized by role.

---

## For Product Managers

### Use case 1 — New feature from idea to tracker in one session

**Scenario:** A PM has a vague feature idea and needs to turn it into a structured GitHub Project with epics, stories, and acceptance criteria.

**Without ehai:** 30-60 minutes writing a PRD in Notion, then manually creating 10-20 GitHub issues with the right labels, fields, and cross-links.

**With ehai (Claude Code or Claude Desktop):**

```
/ehai:plan  Users need to control which push notifications they receive

ehai: What problem does this solve?
PM:   Users complain about too many irrelevant notifications

ehai: Who are the main user types?
PM:   Mobile users (iOS and Android), PM users

ehai: What does success look like in 30 days?
PM:   50% of active users update their notification preferences
      Notification-related support tickets drop 20%

[ehai produces full PRD with Mermaid user flows]
[gap-finder runs: "No error/edge case flow — what happens when saving fails?"]

PM:   Show a toast and retry once automatically

/ehai:breakdown
[PRD → 3 epics, 11 stories, acceptance criteria for each story]

/ehai:sync
[11 items created in GitHub Projects · Status/Priority/Size fields set]
```

Total time: ~15 minutes. Zero manual copy-paste.

---

### Use case 2 — Daily standup status

```
/ehai:status

→ Notifications epic: 73% done (8/11 tasks)
→ Stale items (no update in 5 days): #34 "iOS silent push cert", #41 "Prefs persistence"
→ Open PRs: #22 "Toggle component" (3 days old, no review)
→ Risk: "iOS silent push cert expires in 14 days — assign #34 today"
```

The PM gets this in 2 seconds instead of checking GitHub Projects manually.

---

### Use case 3 — Bug triage during a sprint

```
/ehai:triage  "App crashes when user opens notification settings with an empty preference list"

ehai: Type: Bug  Priority: P1  Size: S

Similar existing items:
  #38 "Crash on empty prefs" — created 3 days ago

Is this the same issue or a new one?
  A. Same — link to #38
  B. New — create a separate item

PM: B

→ Created #51 "Crash on empty notification pref list"
   Linked to #38 as related issue
   Set P1, Size S, assigned to current sprint
```

---

### Use case 4 — PM on Claude Code CLI (no Claude Desktop)

PMs who prefer the terminal get the same experience:

```bash
# in any terminal with Claude Code
claude
> /ehai:plan  A settings screen for notification preferences
```

ehai detects the PM role (set at `ehai init`) and formats output for non-technical readers: plain-language summaries, Mermaid diagrams inline, and an auto-opened HTML report at `~/.ehai/reports/`.

---

## For Developers

### Use case 5 — Start a task without context-switching

```
/ehai:start #42

→ Branch feat/42-notification-toggle created from main
→ Task loaded: "Notification Toggle Component"
   Acceptance criteria:
   - Given I open Settings > Notifications
   - When I toggle a category off
   - Then push notifications stop arriving for that category within 60 seconds
→ Card #42 set to In Progress
```

The developer now has the branch, the full acceptance criteria, and the design notes in context — without leaving their AI harness.

---

### Use case 6 — Open a PR with a generated description

```
/ehai:pr

→ PR title: "feat(notifications): add notification category toggle"
→ Body:
   ## Summary
   Implements the notification toggle component from #42.

   ## Acceptance criteria
   - [x] Toggle updates user preference via API
   - [x] Change takes effect within 60 seconds
   - [ ] Offline state handled (shows retry toast)

→ Card #42 moved to In Review
```

The PR links the issue, maps the diff to the acceptance criteria, and surfaces any unverified criteria so the reviewer knows what to focus on.

---

### Use case 7 — Build an isolated UI prototype

**Scenario:** Trying out two visual approaches for the notification settings screen before committing to one.

```
/ehai:poc new "notification settings screen"

→ Worktree: ~/.ehai/worktrees/my-app/notifications/poc-1
→ Design tokens extracted: Tailwind config, NativeWind theme
→ .env copied, node_modules linked, port 3042 allocated
→ Dev server started at http://localhost:3042
→ Screenshot captured at ~/.ehai/worktrees/.../poc-1/screen-01.png

/ehai:poc iterate poc-1 "Make the section headers sticky and add an 'All Off' button"

→ poc-2 created (lineage: poc-1)
→ Dev server started at http://localhost:3043

/ehai:poc compare poc-1 poc-2

→ Side-by-side gallery: http://localhost:8080/compare
→ Diff summary: +sticky headers, +all-off button, +12 lines of JSX
```

Neither poc-1 nor poc-2 touched `main`. The source repo's working tree is byte-identical to before.

---

### Use case 8 — Monorepo with multiple apps

ehai detects the monorepo structure at `ehai repo init`:

```
Detected: pnpm monorepo
  apps/web      → Vite React (Tailwind CSS)
  apps/mobile   → React Native (NativeWind, Expo Router)
  services/api  → Go (Gin)

Which app does this POC target?
  A. apps/web
  B. apps/mobile
```

Each POC worktree gets the right `.env`, package manager, and port set for its app — including `RCT_METRO_PORT` isolation for React Native's Metro bundler.

---

## For Teams

### Use case 9 — Shared PRD and breakdown across PM and devs

The PM creates the PRD and breakdown in Claude Desktop. The sync writes items to GitHub Projects. Developers use Claude Code to pick up those items:

```
Dev: /ehai:next
     → #47 "Notification API endpoint" (P1, unblocked, assigned to me)
     → Acceptance criteria: POST /users/me/notification-prefs accepts JSON…

Dev: /ehai:start #47
     → branch feat/47-notification-api created
     → acceptance criteria in context
```

The PM's planning work flows directly into the developer's context. No copy-paste, no Slack messages asking "what does this ticket mean?".

---

### Use case 10 — Data policy guard across the team

A developer accidentally pastes a real user email into a task comment:

```
Dev: /ehai:comment_task  #51  "Reported by jane.smith@company.com"

ehai: Sensitive data detected: email ×1
      How should ehai handle this?
      A. Redact → "Reported by [REDACTED:email]"
      B. Keep as-is
      C. Cancel

Dev: A

→ Comment posted: "Reported by [REDACTED:email]"
→ Audit log: { "command": "comment_task", "action": "redact", "findings": [{"kind":"email","count":1}] }
```

The email never reaches GitHub. The audit log records what happened without storing the value.

---

## For OSS Maintainers

### Use case 11 — Triage incoming issues faster

Connect ehai to your open-source project tracker and triage issues from your AI harness:

```
/ehai:triage  "Build fails on Windows with MSVC: error C2440 in src/util.rs"

→ Type: Bug  Priority: P2  Size: M
→ Tags: platform:windows, build
→ Similar: #89 "Windows CI failure on MSVC" (closed 2 weeks ago — may be related)

Is this new or the same?
  A. New  B. Duplicate of #89

→ Created #124, assigned "help wanted" label
```

---

## Adapting ehai to your workflow

**Change your role**

```bash
ehai config set user.role pm      # pm | dev | both
```

**Rename all commands**

```bash
# in ~/.ehai/config.toml
[brand]
name = "acme"

ehai adapters regenerate    # all slash commands become /acme:plan, /acme:start, etc.
```

**Multiple trackers / projects**

```bash
ehai tracker set github              # switch to GitHub Projects
ehai tracker set local               # switch to local store (offline)
```

**Offline mode**

The local tracker (`ehai tracker set local`) supports all PM flows — plan, breakdown, status, triage — with no network required. Use it for internal planning, pre-sales work, or when GitHub is unavailable.
