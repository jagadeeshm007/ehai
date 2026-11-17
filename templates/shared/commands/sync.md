# Sync a plan to the tracker

PRD: $ARGUMENTS

1. If no PRD was named, call `list_plans` and ask which one.
2. Call `sync_plan` with `apply: false` to preview. Show what will be **created**, **updated** and left **unchanged**, plus any items on the board that are no longer in the plan (they are never deleted).
3. If nothing would change, say so and stop.
4. Ask for confirmation with the question tool: **Sync now (Recommended)** · Not yet.
5. On yes, call `sync_plan` with `apply: true`. Report the result with links to the created items, and any items that failed and why.
6. Re-running is always safe: items are matched by a hidden id, so nothing is duplicated, and cards that people moved stay where they are.

If the tool says no tracker is connected, tell the user to run `{{brand}} tracker set` in a terminal.

{{role_block}}

{{ask_block}}
