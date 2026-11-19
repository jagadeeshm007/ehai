# {{brand}} AI-SDLC Assistant

{{brand}} v{{version}} connects your AI harness to your GitHub Projects tracker
for product planning, task management, and UI proof-of-concepts.

## MCP server

To use {{brand}} in any MCP-compatible harness, add this to your MCP config:

```json
{
  "mcpServers": {
    "{{brand}}": {
      "command": "{{brand}}",
      "args": ["mcp"]
    }
  }
}
```

After adding it, run `{{brand}} doctor` to verify the connection.

## Commands

| Command | Description |
|---------|-------------|
| `/{{prefix}}plan [idea]` | Turn an idea into a PRD (interviews the user, Mermaid diagrams) |
| `/{{prefix}}breakdown [prd]` | Break a PRD into epics, stories and tasks with acceptance criteria |
| `/{{prefix}}sync [prd]` | Push an approved breakdown to the tracker (idempotent) |
| `/{{prefix}}status` | Progress, blockers, stale work and risks |
| `/{{prefix}}triage [report]` | Classify a bug or request and create a tracker item |
| `/{{prefix}}assign [request]` | Assign, reassign or take a task |
| `/{{prefix}}prefs` | Show and edit your role and preferences |
| `/{{prefix}}help` | This overview |

## MCP tools

The `{{brand}}` MCP server exposes: `ping`, `save_prd`, `get_plan`, `list_plans`,
`save_breakdown`, `sync_plan`, `status_report`, `triage`, `assign_task`,
`comment_task`, `team`, `guard_check`.

Resource: `ehai://prefs` — your role (`pm`/`dev`/`both`) and learned preferences.

## Data policy guardrails

This tool is designed for teams that handle sensitive data. Never put real
personally identifiable information (names with identifiers, dates of birth,
ID numbers, phone numbers, emails, or addresses) in plans, tasks, mock data
or examples. `{{brand}}` tools check what they write and will ask if they
detect something suspicious.

Run `{{brand}} doctor` if tools are not responding.

{{insights_block}}
