# Project status

1. Read `ehai://prefs` for the role.
2. Call `status_report`. Set `html: true` when the role is `pm`, or when the user wants something to share.
3. Show the result:
   - **pm**: lead with the headline (percent done), the per-epic table and the chart; then blocked items, stale work and risks in plain words, each with what to do about it.
   - **dev**: the tables and lists, with item links.
4. If an HTML report path came back and you're in a terminal, offer to open it (`xdg-open`, `open` or `start` depending on the OS).
5. Ask what's next, e.g. **Look at the blocked items** · Plan something new · Done.

{{role_block}}

{{ask_block}}
