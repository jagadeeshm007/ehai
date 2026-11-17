# Plan — project-admin roles and the team list

Status: **approved** · 2026-11-17

## Why

- **Roles follow real authority.** Whoever administers the GitHub project is the PM side; everyone else is a developer. Only admins choose their role (`pm`, `dev` or `both`); non-admins become `dev` automatically, with no question.
- **The team list powers collaboration:** assigning and reassigning tasks, taking your own, and @mentioning people. It's cached so ehai doesn't ask GitHub every time.

## Decisions (from your answers)

| Topic | Decision |
|---|---|
| Who is "admin" | `ProjectV2.viewerCanClose` is true (GitHub lets only project admins close a project); the owner of a personal project is always admin |
| No project connected yet | `dev`; the role question appears once they connect a project they administer |
| Team | Repo **assignable users** ∪ **org members** (org-owned projects), merged by login and marked `assignable` |
| PM-only actions | **None.** Everyone can do everything. The role sets the experience (tone, suggested flows, defaults), not permissions; GitHub permissions stay the real gate |
| Cache | Filled on the first GitHub call, auto-refreshed when older than 24 h, `ehai team refresh` anytime |

**Assumption to confirm:** "control the env" means the role is *managed* (admins choose, others are dev), not that ehai blocks devs from anything.

## Design

### 1. Role resolution — `ehai-core` + CLI
- New config section, written by ehai rather than by hand:
  ```toml
  [access]
  project_admin = true          # last check
  checked = "2026-11-17"
  ```
- **Effective role:**
  - Admin: the chosen `role` (`pm` / `dev` / `both`). Once the project is connected, the default is `pm`.
  - Non-admin or no project: `dev`.
  - A session override (`EHAI_ROLE`) only applies to admins, so it can't be used to bypass the rule.
- **When admin is checked** (one cheap GraphQL call):
  - `init`, after the tracker step. The tracker question now comes **before** the role question, and the role question is shown **only to admins**.
  - `tracker set`, after connecting. Admins are asked for a role if they don't have one; non-admins see "You're a developer on *project*".
  - The daily team refresh and `ehai team refresh`. If someone loses admin, they're downgraded to `dev` with a one-line notice. If they gain it, they're told they can choose (`ehai role set`).
- **New commands:**
  - `ehai role` shows the role and why ("admin of jagadeeshm007 #2 → chose pm").
  - `ehai role set pm|dev|both` is for admins; others get a clear explanation.
- `doctor` shows the role, why it applies, and the team cache age.
- The `ehai://prefs` resource adds `role_source` and `project_admin`, so skills phrase things correctly.

### 2. Team list — `ehai-tracker::team`
- **GitHub queries** (read-only):
  - `repository.assignableUsers` (paginated: login, name, node id)
  - `organization.membersWithRole`, only when the project owner is an org (needs `read:org`, which you have)
  - `ProjectV2.viewerCanClose`, fetched in the same request
- **Merged member:** `{ login, name, id, assignable: bool, sources: [repo, org] }`.
- **Cache file:** `~/.ehai/cache/team/<owner>-<project>.json`, holding `{ fetched_at, viewer, viewer_is_admin, members[] }`.
  - Written atomically under a lock.
  - When it's stale, a refresh runs. If the refresh fails, ehai keeps using the old list and warns.
- **CLI:** `ehai team` lists members and the cache age; `ehai team refresh` forces a fetch and also re-checks admin.

### 3. Assigning and mentions — tracker + MCP
- **`Tracker` trait gains:**
  - `assign(id, logins)` and `unassign(id, logins)`
  - `Item.assignees`
- **GitHub implementation:**
  - Issues use `addAssigneesToAssignable` / `removeAssigneesFromAssignable`, with user node ids from the team cache.
  - Draft items use their own assignee update. I'll verify by schema introspection before building it.
- **Mentions:** `comment(id, body)` stays the same; mention lists are added as `@login …` at the top.
- **New MCP tools:**
  - **`team`** lists members, with an optional refresh.
  - **`assign_task`**:
    - It takes `{ item, assignees[], mode: add | replace | remove }`.
    - `item` accepts `#18`, a URL, an ehai id, or a title fragment; `"me"` means the caller.
    - If a name is unknown or ambiguous, or the item is ambiguous, it returns `needs_input` with team or item options.
    - It warns when someone isn't assignable on the repo.
  - **`comment_task`** posts a comment with optional mentions.
- **Plans:**
  - Stories and tasks get an optional `assignees` field.
  - Assignees are set **when an item is created**. After that the board owns them, like status, so a re-sync never undoes a reassignment.
  - The breakdown preview gets an "Assignee" column.
- **Triage:** an optional assignee.
- **Status report:**
  - A "By person" table (open / in progress / done per assignee).
  - New risk flags: in-progress work with nobody assigned, and anyone with more than 5 items in progress.

### 4. Skills and prompts
- New **`/ehai:assign`** ("assign #18 to Priya", "I'll take #19", "move #20 from Ravi to Asha"). It asks through the question tool, offering team members as options.
- `breakdown` suggests assignees from the team (optional). `status` shows the per-person table. `triage` can assign.
- `init`'s "Try next" card and `help` change by role: PMs see plan, breakdown and status; devs see assign/take, status and triage.

## Tests
- **Fake GitHub:**
  - `viewerCanClose` switchable per test
  - assignable users and org members
  - assign and unassign mutations
  - assignees in items
- **Contract suite:** assign/unassign round-trip on the memory and GitHub trackers; unknown login → error.
- **Role:**
  - Admin is asked the role; non-admin gets no question and `dev`.
  - No project → `dev`.
  - Losing admin on refresh downgrades; `role set` is refused for non-admins.
  - `EHAI_ROLE` is ignored for non-admins.
  - `init` with a non-admin asks one question fewer.
- **Team cache:** first call fills it; a fresh cache makes no network call; a stale one refreshes; a failed refresh keeps the old list.
- **MCP:** `assign_task` covers "me", replace and remove, an ambiguous name (`needs_input`), an unassignable user (warning), and by-person status numbers.
- **Live (opt-in):** on aisdlc-demo, assign #19 to yourself, unassign, and post a mention comment. The non-admin path can only be tested on the fake, because you administer everything here.

## Out of scope (later)
- Notifying people outside GitHub (Slack, email).
- Workload-based assignment suggestions (M6, preference learning).
- The developer flow (`/ehai:next`, `start`, `pr`, `done`) builds on this: `next` will prefer tasks assigned to you.

## Deliverable
- One commit: `feat: project-admin roles, team list, task assignment`.
- Docs updated (README, PROGRESS, a DECISIONS ADR).
- Everything above tested, plus the live assign check on aisdlc-demo.
