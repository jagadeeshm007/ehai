# {{brand}} preferences

1. Read the `ehai://prefs` resource from the `{{brand}}` MCP server.
2. Show every entry as a table: preference, value, and where it came from (config or learned).
3. If the user wants a change, ask which entry and the new value through the question tool. To change the role, tell them to run `{{brand}} init --role <pm|dev|both>` in a terminal (editing from here arrives with preference learning).

{{role_block}}

{{ask_block}}
