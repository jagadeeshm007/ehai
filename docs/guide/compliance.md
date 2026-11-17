# Data Policy

ehai scans what it writes — tracker items, PRDs, POC mock data, reports — and surfaces or blocks sensitive data before it lands anywhere. The behavior is configurable per team and per domain.

## Domain profiles

The guard uses **additive profiles**. The `tech` profile is always the base; each other profile adds domain-specific patterns on top.

| Profile | What it detects |
|---|---|
| `tech` *(default)* | SSN/national IDs, email, phone, street address, date of birth |
| `health` | *+ tech* · medical record numbers (MRN), insurance/member IDs, name near medical terms |
| `finance` | *+ tech* · credit card numbers (Luhn-validated), IBAN, US bank routing numbers |
| `gdpr` | *+ tech* · EU passport patterns, data-subject language (consent, right to erasure) |

## Modes

| Mode | Behavior |
|---|---|
| `off` | No scanning. Write anything. |
| `warn` *(default)* | Surface findings and ask: **redact** / **keep** / **cancel** before writing. |
| `strict` | High-confidence findings are auto-redacted. Low-confidence findings are surfaced. Never writes suspicious data silently. |

## Configuration

```toml
# ~/.ehai/config.toml
[compliance]
mode    = "warn"    # off | warn | strict
profile = "tech"    # tech | health | finance | gdpr
```

Change at any time — takes effect on the next tool call. No restart needed.

## Custom patterns

Append extra patterns after the profile rules:

```toml
[compliance]
mode    = "warn"
profile = "tech"
patterns = [
  { name = "employee-id", regex = "\\bEMP-\\d{6}\\b" },
  { name = "project-code", regex = "\\bPROJ-[A-Z]{3}-\\d{4}\\b" },
]
```

Custom patterns are matched with the same confidence logic as built-in rules. `name` becomes the finding kind shown in redaction notices: `[REDACTED:employee-id]`.

## Audit log

Every compliance decision is appended to `~/.ehai/audit/` as newline-delimited JSON:

```jsonc
{
  "ts": "2026-11-18T10:23:01Z",
  "command": "save_prd",
  "action": "redact",
  "findings": [{ "kind": "email", "count": 2 }]
}
```

!!! note "Values are never logged"
    The audit log records **kinds and counts** only — never the actual sensitive values. `ehai guard-check "<text>"` echoes findings to your terminal but never writes the content to any file.

## Changing profile mid-session

```bash
ehai config set compliance.profile health
```

Or set it once in `~/.ehai/config.toml`. A repo-level `.ehai.toml` can override the global setting for a specific project.

## Checking a string manually

```bash
ehai guard-check "SSN: 123-45-6789 and email jane@example.com"
# findings: ssn ×1, email ×1
```

Useful for testing a custom pattern before writing it to config.
