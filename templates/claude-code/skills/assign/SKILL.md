---
name: assign
description: Assign, reassign or unassign a tracker item, or take it yourself. Use for "assign #18 to Priya", "I'll take #19", "move #20 from Ravi to Asha", or adding a mention comment.
argument-hint: [who gets what]
---

# Assign a task

Request: $ARGUMENTS

1. Work out the item (`#18`, a URL, or a description) and who it should go to —
   a login, "me", or a name to match against the team.
2. Call `team` if you need to resolve a name you're not sure about.
3. Call `assign_task` with the item and assignees:
   - "assign to X" / "I'll take it" → `mode: "add"` (the default).
   - "move from X to Y" / "reassign to Y" → `mode: "replace"` with just the new assignees.
   - "unassign X" / "take X off this" → `mode: "remove"`.
   If it returns `needs_input` (ambiguous item or unmatched name), ask the user to pick and call again.
4. If the user also wants to notify someone or leave a note, call `comment_task` with `mentions` set.
5. Report what happened with a link to the item.

{{role_block}}

{{ask_block}}
