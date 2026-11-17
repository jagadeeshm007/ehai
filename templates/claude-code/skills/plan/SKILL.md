---
name: plan
description: Turn a product idea into a PRD (problem, users, goals, scope, user flows, screens, success metrics, risks) by interviewing the user in plain language. Use when the user wants to plan a feature, write requirements or a PRD.
argument-hint: [idea]
---

# Plan a feature with {{brand}}

Idea from the user: $ARGUMENTS

1. Read the `ehai://prefs` resource for the user's role.
2. Interview the user with the question tool. Usually two rounds of at most 4 questions:
   - who it's for and what goes wrong for them today,
   - what success looks like, with a number (e.g. "60% of bookings online in 3 months"),
   - what is out of scope, and which screens matter most.
   Offer concrete options based on what you already know. Skip anything you can infer.
3. Write the PRD: problem, users, goals, scope, out of scope, user flows as Mermaid `flowchart` diagrams, short screen descriptions, success metrics (each with a number), risks with mitigations.
4. Call the `save_prd` tool. Pass `repo_path` when you're working inside a repository. If it returns `problems`, fix them and call again.
5. Present the PRD:
   - In Claude desktop or claude.ai: create it as a document (artifact) the user can read and share.
   - In a terminal: show a short summary, the user-flow diagram and the saved file path.
6. Ask what's next: **Break it into tasks (Recommended)** · Change something · Stop here. To break it down, follow `/{{prefix}}breakdown` with this PRD.

{{role_block}}

{{ask_block}}
