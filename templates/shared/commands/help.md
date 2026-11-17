# {{brand}} help

1. Call the `ping` tool of the `{{brand}}` MCP server. If it fails, tell the user {{brand}} is not reachable and suggest running `{{brand}} doctor` in a terminal, then stop.
2. Read the `ehai://prefs` resource to learn the user's role.
3. Explain {{brand}} in three short bullets for that role:
   - plan product requirements and break them into tracked tasks
   - track progress and issues in the team tracker (GitHub Projects by default)
   - build UI proof-of-concepts that match the app's design, isolated from `main`
4. Show the commands available today as a table:

   | Command | What it does |
   |---|---|
   | `/{{prefix}}plan` | Turn an idea into a PRD |
   | `/{{prefix}}breakdown` | Break a PRD into epics, stories and tasks |
   | `/{{prefix}}sync` | Put the plan on the tracker board (no duplicates) |
   | `/{{prefix}}status` | Progress, blockers and risks |
   | `/{{prefix}}triage` | Turn a bug or request into a tracker item |
   | `/{{prefix}}assign` | Assign, reassign or take a task, or mention someone |
   | `/{{prefix}}prefs` | Your role and preferences |
   | `/{{prefix}}help` | This overview |

5. Ask what they want to do next through the question tool.

{{role_block}}

{{ask_block}}
