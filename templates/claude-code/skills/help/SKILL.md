---
name: help
description: Explain what {{brand}} can do and check that it is connected. Use when the user asks what {{brand}} is, how to start, or which `/{{prefix}}…` commands exist.
disable-model-invocation: false
---

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
   | `/{{prefix}}help` | This overview |
   | `/{{prefix}}prefs` | Show your role and preferences |

   Planning, tracking and POC commands are coming in the next releases.
5. Ask what they want to do next through the question tool.

{{role_block}}

{{ask_block}}
