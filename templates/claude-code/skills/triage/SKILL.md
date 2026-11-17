---
name: triage
description: Turn a bug report or request into a classified, prioritised tracker item, checking for duplicates first. Use when the user reports a problem or asks for something new.
argument-hint: [description]
---

# Triage a report

Report: $ARGUMENTS

1. If the report is vague, ask at most 3 questions with the question tool: what happened vs. what was expected, how often / how many people, where (screen, platform).
2. Decide, and explain briefly:
   - type: **Bug**, **Story** (new behaviour) or **Task**,
   - priority: P0 (patients or safety blocked, no workaround) · P1 (major, workaround exists) · P2 (minor) · P3 (cosmetic),
   - size XS–XL if you can tell.
3. Write a clear title and a description with steps to reproduce (for bugs) or acceptance criteria (for stories). No real patient data.
4. Call `triage`. If it returns `needs_input` because similar items exist, ask the user whether it's one of those or new, and call again with their choice.
5. Report what happened with a link to the item.

{{role_block}}

{{ask_block}}
