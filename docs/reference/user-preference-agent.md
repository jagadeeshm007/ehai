# User Preference Agent Design

ehai learns how each user and repo works over time and injects that knowledge into every tool call, so later interactions need fewer questions and produce better-tailored output by default.

---

## What ehai already has

The `ehai://prefs` MCP resource is loaded at the start of every skill invocation. It reflects `~/.ehai/prefs/user.toml` at call time — there is no in-memory session state to sync. The AI sees a compact "User preferences" block such as:

```
planning_style: detailed
last_plan_complexity: 7
poc_created: true
```

Preferences have a `source` (`explicit` = user-stated, `observed` = inferred from behavior), a `confidence` (0.0–1.0), and timestamps. Explicit statements always beat observations. Confidence decays if a preference is not reinforced; contradicting evidence lowers it.

---

## Maintaining context across sessions

The MCP resource is stateless by design: it reads the TOML file on every call. The AI does not need to "remember" across sessions because the prefs file is the persistent memory. No special session handoff is required; `ehai://prefs` always reflects the latest state.

---

## Progressive signal gathering

After every significant interaction, MCP tools call a private `observe_interaction` helper on `EhaiServer`. It is fire-and-forget: errors are silently ignored so a prefs write failure never breaks the main flow.

```rust
fn observe_interaction(&self, key: &str, value: &str) {
    let mut prefs = Prefs::load_global(&self.ctx.home).unwrap_or_default();
    prefs.set(key, value, Scope::Global, Source::Observed);
    let _ = prefs.save_global(&self.ctx.home);
}
```

**Current observation points:**

| Tool | Key | Value |
|---|---|---|
| `save_prd` | `last_plan_complexity` | count of risks + flows as a string |
| `poc_new` | `poc_created` | `"true"` |

Candidates for future wiring:
- `plan_explore` returning 0 gaps → `requirements_completeness: thorough`
- Three or more `poc_iterate` calls on the same feature → `poc_iteration_depth: deep`
- Consistent story-point scale chosen → `estimate_scale: fibonacci`

---

## Compaction

When the prefs file grows beyond 50 entries, `save_global` runs a compaction pass automatically:

1. Keep all entries where `source == explicit` (user-stated facts are never dropped).
2. Keep the top-30 observed entries, sorted by key (deterministic across machines).
3. Write the compacted set back.

This bounds the file at roughly 50–60 entries indefinitely.

---

## Privacy guarantee

Observation keys and values describe behavioral patterns only — they never contain PHI. The PHI guard (`ehai-guard`) does not need to scan this file because:

- Keys are hard-coded strings in source (`"last_plan_complexity"`, `"poc_created"`).
- Values are counts, booleans, or enum-like labels — never user-entered free text.
- The prefs file is never sent to the tracker, included in reports, or read by the LLM as raw content (only the compact `to_context_text()` block is injected).

Values are never logged in compliance findings — only kinds and counts.

---

## Two scopes

| Scope | File | Committed? |
|---|---|---|
| Global (user) | `~/.ehai/prefs/user.toml` | No — machine-local |
| Repo (project) | `<repo>/.ehai/prefs/project.toml` | Configurable |

Resolution order: explicit repo decision > explicit user preference > observed repo convention > observed user preference > defaults.

---

## User control

`ehai prefs list` — show all entries with source, confidence, and timestamps.  
`ehai prefs set <key> <value>` — write an explicit entry that beats all observations.  
`ehai prefs delete <key>` — remove an entry.  
`prefs.learning = false` in config — disable all observation writes globally.
