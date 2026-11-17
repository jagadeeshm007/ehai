# Config Reference

ehai uses TOML configuration with a layered merge. Narrower layers win:

```
defaults < ~/.ehai/config.toml < .ehai.toml (repo) < --config flag < EHAI_SECTION__KEY env < CLI flags
```

---

## `[brand]`

```toml
[brand]
name = "ehai"   # rename the binary and all slash commands; default "ehai"
```

After changing `name`, run `ehai adapters regenerate` to re-render all installed harness files.

---

## `[compliance]`

```toml
[compliance]
mode    = "warn"    # off | warn | strict
profile = "tech"    # tech | health | finance | gdpr
patterns = []       # custom patterns appended after profile rules
```

**`patterns` schema:**

```toml
[[compliance.patterns]]
name  = "employee-id"           # used in [REDACTED:name] notices
regex = "\\bEMP-\\d{6}\\b"     # Rust regex syntax
```

See [Data Policy guide](../guide/compliance.md) for profile details.

---

## `[sandbox]`

```toml
[sandbox]
mode = "restricted"   # off | restricted | container
```

| Mode | What it restricts |
|---|---|
| `off` | No restrictions |
| `restricted` *(default)* | POC agents write only inside their worktree and `~/.ehai/`; destructive commands blocked |
| `container` | POC dev servers run in Docker/Podman; requires a container runtime |

---

## `[update]`

```toml
[update]
channel = "alpha"   # alpha | beta | stable
auto    = false     # true: auto-install in background (default: notify only)
check   = true      # false: disable background update checks entirely
```

---

## `[tracker]`

```toml
[tracker]
type    = "github"         # github | local
project = "My Project"     # GitHub Projects name or local store label
org     = "my-org"         # GitHub org or user; auto-detected from remote if unset
```

Switch trackers with `ehai tracker set github` or `ehai tracker set local`.

---

## `[env]`

Controls worktree environment provisioning (POC engine and `ehai env sync`):

```toml
[env]
strategy      = "copy"     # copy | symlink
include       = []         # extra glob patterns to copy (e.g. "apps/*/.env*")
exclude       = []         # patterns to skip (e.g. ".env.production")
install_deps  = true       # run package manager install in each worktree
rn_native     = false      # pod install / Gradle (slow; opt-in)

[env.smoke]
web = "pnpm -C apps/web typecheck"
api = "go build ./..."
```

---

## `[prefs]`

```toml
[prefs]
learning = true    # false: disable preference observation entirely
```

---

## `[log]`

```toml
[log]
level  = "info"    # error | warn | info | debug | trace
rotate = true      # rotate logs daily; keep last 7 days
```

Logs are written to `~/.ehai/logs/` as JSON.

---

## Environment variable overrides

Any config key can be overridden with `EHAI_<SECTION>__<KEY>`:

```bash
EHAI_COMPLIANCE__MODE=strict ehai init
EHAI_UPDATE__CHANNEL=beta ehai update
```

Values are parsed as TOML literals (`true`, `42`, `"string"`).

---

## Schema

```bash
ehai config schema          # print the JSON schema for config.toml
ehai config show            # print the effective merged config
ehai config path            # print the global and repo config paths
```
