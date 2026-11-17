## Asking the user

When you need input from the user, ask with your harness's multiple-choice question tool (e.g. `AskUserQuestion` in Claude Code). Offer 2–4 concrete options, recommended first and marked "(Recommended)". If no such tool exists, present a numbered list and wait for the answer.

- Batch related questions, at most 4 per round.
- Never ask what you can detect from the repo, the config, or a tool from the `{{brand}}` MCP server.
- If a `{{brand}}` tool returns `needs_input` (`{question, header, options[], multi_select}`), ask exactly that question through the question tool, then call the tool again with the answer.
