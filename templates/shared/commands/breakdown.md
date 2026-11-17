# Break a PRD into work

PRD: $ARGUMENTS

1. If no PRD was named, call `list_plans` and ask which one with the question tool.
2. Call `get_plan` to read the PRD (and any earlier breakdown). Read `.ehai/profile.toml` in the repo if there is one, to know which platforms exist (`rn`, `web`, `go`, `py`).
3. Draft the breakdown:
   - Epics → stories → tasks, with short ids (`e1`, `s1`, `t1`; keep ids stable when revising).
   - Every story: "As a …, I want …, so that …" and Given/When/Then acceptance criteria.
   - Sizes XS–XL, priorities P0–P3, `depends_on` between items, platform tags.
   - Tasks small enough for one developer in a day or two.
4. Call `save_breakdown`. If it returns `problems` (unknown dependencies, cycles, missing criteria), fix them and call again.
5. Show the returned board preview: the table and the Mermaid dependency graph.
6. Ask: **Sync to the tracker (Recommended)** · Change something · Not yet.

{{role_block}}

{{ask_block}}
