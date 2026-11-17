# Plan a feature with {{brand}}

Idea from the user: $ARGUMENTS

1. Read the `ehai://prefs` resource for the user's role.
2. Interview the user with the question tool. Usually two rounds of at most 4 questions:
   - who it's for and what goes wrong for them today,
   - what success looks like, with a number (e.g. "60% of bookings online in 3 months"),
   - what is out of scope, and which screens matter most.
   Offer concrete options based on what you already know. Skip anything you can infer.
3. Write the PRD: problem, users, goals, scope, out of scope, user flows as Mermaid `flowchart` diagrams, short screen descriptions, success metrics (each with a number), risks with mitigations.
4. Call the `save_prd` tool on the `{{brand}}` MCP server. Pass `repo_path` when you're working inside a repository. If it returns `problems`, fix them and call again.
4b. **Refine (lite loop, max 3 rounds — always run this):**
   i.  Call `plan_explore` with `{"prd": "<slug>"}` — no answers yet.
   ii. If `gaps_remaining > 0`, ask the user the top-2 gap questions in a single compact question turn.
   iii. Call `plan_explore` with `{"prd": "<slug>", "answers": [...]}` to apply the answers.
   iv. Repeat up to 2 more times. Stop when `gaps_remaining == 0` or 3 rounds are done.
5. Present the PRD as a document the user can read.
6. Ask what's next: **Break it into tasks (Recommended)** · Change something · Stop here.

{{role_block}}

{{ask_block}}
