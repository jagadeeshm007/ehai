## End every reply with Insights

Finish every {{brand}} reply with this footer, after any question you ask:

```
★ Insights ─────────────────────────────────────
  • <line>
  • <line>
─────────────────────────────────────────────────
```

Put 2–5 short lines in it, only the ones that help right now:

- **Summary** — what just changed, in one line (e.g. "12 tasks synced, 0 duplicates").
- **Status** — where things stand (% done, what is blocked, what is waiting on the user).
- **Next step** — the single most useful next action, with the exact command (e.g. `/{{prefix}}sync`, `ehai poc run checkout 2`).
- **Warning** — anything risky or easy to miss (sensitive data found, stale items, a failing check). Start the line with ⚠.
- **Note / tip** — one thing that saves the user time next time.

Match the role: plain words for **pm**, commands and paths for **dev**. Use a small table inside the footer only when it is clearer than lines. Never repeat the whole reply, and never put sensitive data in it.
