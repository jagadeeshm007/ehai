## Asking the user

When you need input from the user, ask with your harness's multiple-choice question tool (e.g. `AskUserQuestion` in Claude Code). Offer 2–4 concrete options, recommended first and marked "(Recommended)". If no such tool exists, present a numbered list and wait for the answer.

- Batch related questions, at most 4 per round.
- Never ask what you can detect from the repo, the config, or a tool from the `{{brand}}` MCP server.
- If a `{{brand}}` tool returns `"status": "needs_input"`, ask exactly its `question` with its `options` through the question tool. Then call **the same tool again with the same arguments**, plus `retry_param` set to the chosen option's `value`.

## Patient data

This is a health-tech product. Never put real patient data in plans, tasks, mock data or examples — no real names with conditions, dates of birth, record numbers, member ids, phone numbers, emails or addresses. Use obviously made-up examples ("Test Patient A"). `{{brand}}` tools check what they write and will ask if they find something.
