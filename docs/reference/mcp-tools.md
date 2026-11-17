# MCP Tools Reference

ehai exposes an MCP server (`ehai mcp`) with the following tools. All tools are available in every connected harness.

When a tool returns `"status": "needs_input"`, the harness's question UI shows the prompt; call the same tool again with `retry_param` set to the chosen value.

---

## Core

| Tool | Description |
|---|---|
| `ping` | Health check — returns `{"ok": true, "version": "…"}` |

---

## PM flow

| Tool | Key params | Description |
|---|---|---|
| `save_prd` | `title`, `content`, `slug?`, `policy?` | Store a PRD; runs the data-policy guard before writing |
| `get_plan` | `slug` | Retrieve a saved PRD by slug |
| `list_plans` | — | List all saved PRDs with slug, title, created_at |
| `save_breakdown` | `slug`, `breakdown`, `policy?` | Store an epic/story/task breakdown linked to a PRD slug |
| `sync_plan` | `slug` | Push the breakdown to the connected tracker, idempotently |
| `status_report` | `slug?` | Progress report: % done per epic, blockers, stale items, open PRs |
| `triage` | `report`, `policy?` | Classify a bug/request → tracker item with duplicate detection |
| `plan_explore` | `slug`, `round?` | Gap-finder iteration: surfaces unanswered questions in a PRD draft |

---

## Developer flow

| Tool | Key params | Description |
|---|---|---|
| `comment_task` | `item_id`, `body`, `policy?` | Post a comment on a tracker item |
| `assign_task` | `item_id`, `assignee` | Assign a tracker item to a team member |

---

## Team

| Tool | Key params | Description |
|---|---|---|
| `team` | `action` (`list`/`add`/`remove`) | Manage the team roster (GitHub assignable users, with cache) |

---

## Guard

| Tool | Key params | Description |
|---|---|---|
| `guard_check` | `text`, `mode?` | Scan a string for sensitive data; returns findings with kind and count. Values are never stored. |

---

## Resources

| Resource URI | Description |
|---|---|
| `ehai://prefs` | Current user role and learned preferences as a compact JSON block |

---

## Sensitive data policy param (`policy`)

Tools that write content accept an optional `policy` field to answer a pending sensitive-data question:

```jsonc
{
  "tool": "save_prd",
  "args": {
    "slug": "notifications",
    "content": "…",
    "policy": "redact"   // "redact" | "keep" | "cancel"
  }
}
```

If a tool finds sensitive data and `policy` is not set, it returns `needs_input` with a question for the user. Set `policy` on the retry call to proceed.
