# How ehai works

This page covers why ehai exists, what problem it solves, the ideas behind it, how it is built, and how a request moves through it, with diagrams.

**Diagram colour key:** used the same way on every diagram below.

```mermaid
flowchart LR
  U["👤 People"]:::user --> H["🤖 AI harness"]:::harness --> E["⚙️ ehai engine"]:::engine --> X["🌐 External service"]:::ext
  E --> S["💾 Local state"]:::store
  E --> G["🛡️ Safety / guard"]:::guard
  classDef user fill:#f97316,stroke:#c2410c,color:#fff
  classDef harness fill:#2563eb,stroke:#1e40af,color:#fff
  classDef engine fill:#7c3aed,stroke:#5b21b6,color:#fff
  classDef ext fill:#16a34a,stroke:#166534,color:#fff
  classDef store fill:#0891b2,stroke:#155e75,color:#fff
  classDef guard fill:#dc2626,stroke:#991b1b,color:#fff
```

---

## 1. The problem

AI coding assistants made *writing code* fast. Everything around the code is still slow and manual:

| Pain | Who feels it | What it costs |
|---|---|---|
| Requirements live in docs and chats; someone retypes them into the tracker by hand | PM | Hours per feature, lost detail, duplicate issues |
| Every AI tool (Claude Code, Codex, Cursor, Gemini CLI…) needs its own setup, prompts and conventions | Developers | Inconsistent output; the team cannot share one workflow |
| A developer jumps between tracker, terminal and AI chat to start a task, open a PR and move the card | Developers | Context switching; stale boards |
| Trying a UI idea means a branch in the real repo, and a fresh worktree will not run (no `.env`, no deps, port clashes) | PM + Dev | Prototypes are slow, risky to `main`, or never built |
| Prototypes ignore the app's real colors, fonts and components | PM | POCs look nothing like the product, so feedback is poor |
| Someone pastes a customer email or ID into a ticket | Everyone | Compliance incidents |
| The AI asks the same questions every session | Everyone | It never learns how this team works |

## 2. How ehai helps

ehai is **one engine** that every AI harness talks to. It turns the AI from a code generator into a full **SDLC teammate**:

```mermaid
flowchart LR
  idea["💡 Idea"]:::user --> prd["📄 PRD<br/>/ehai:plan"]:::engine
  prd --> bd["🗂️ Breakdown<br/>epics → stories → tasks<br/>/ehai:breakdown"]:::engine
  bd --> board["📋 GitHub Project<br/>/ehai:sync"]:::ext
  board --> start["🌿 Branch + In Progress<br/>/ehai:start"]:::harness
  start --> pr["🔀 PR mapped to criteria<br/>/ehai:pr"]:::harness
  pr --> done["✅ Done<br/>/ehai:done"]:::ext
  bd -. "try the UI first" .-> poc["🧪 POC worktree<br/>/ehai:poc new"]:::engine
  poc -. "promote on request" .-> pr
  board --> status["📊 Status report<br/>/ehai:status"]:::user
  classDef user fill:#f97316,stroke:#c2410c,color:#fff
  classDef harness fill:#2563eb,stroke:#1e40af,color:#fff
  classDef engine fill:#7c3aed,stroke:#5b21b6,color:#fff
  classDef ext fill:#16a34a,stroke:#166534,color:#fff
```

- **PMs** go from idea → PRD → approved tasks on the board in one conversation, in the Claude desktop app or Claude Code, without terminal commands.
- **Developers** pick, start, ship and close tasks from whichever AI tool they like. The branch, PR text and card moves are done for them.
- **Everyone** can ask for a UI prototype that uses the app's own design system and runs in an isolated copy of the repo, so `main` is never touched.
- **The guard** checks every tracker write, PRD and mock-data fixture for sensitive data first.
- **Preferences** make each session a little better than the last.

---

## 3. Philosophy

These rules decide every design trade-off in ehai.

| Principle | What it means in practice |
|---|---|
| **One engine, thin adapters** | All logic is in one Rust binary. Each harness gets only generated config and prompt files that call the same MCP tools, so a new AI tool costs one small adapter, not a rewrite. |
| **Main is sacred** | POCs live in separate worktrees on `ehai/poc/...` branches outside the repo. Nothing is merged, pushed or promoted unless you ask. |
| **Never break, always run** | Atomic updates with automatic rollback, versioned config with migrations, additive-only MCP schemas, and a per-request panic guard so one bad call never kills the server. |
| **Ask, don't guess** | Questions go through the harness's native multiple-choice UI. Anything that can be detected is detected instead of asked. Every question has a default. |
| **Learn and adapt** | Observed choices become preferences, which become defaults. Explicit statements always beat observations, and the user can edit or delete any of them. |
| **Safe by default** | Data guard on every write, sandboxed agent work, secrets never sent to the tracker, logs or the LLM (keys only, never values). |
| **Minimal footprint** | One static binary. No Node, Python or Docker to run it. No telemetry. `ehai uninstall` leaves no traces. |
| **Role-aware** | Same commands for everyone; PMs get plain language and visuals, developers get paths, diffs and commands. |

---

## 4. How it is built

### 4.1 Architecture

```mermaid
flowchart TB
  subgraph people["People"]
    pm["👩‍💼 PM"]:::user
    dev["👨‍💻 Developer"]:::user
  end

  subgraph harnesses["AI harnesses (thin adapters)"]
    cd["Claude Desktop<br/>skills"]:::harness
    cc["Claude Code<br/>plugin + /ehai: commands"]:::harness
    oth["Codex · Cursor · Gemini CLI<br/>OpenCode · VS Code · Windsurf"]:::harness
  end

  subgraph engine["ehai engine — one static Rust binary"]
    mcp["ehai-mcp<br/>MCP server over stdio"]:::engine
    cli["ehai-cli<br/>init · doctor · update"]:::engine
    planner["ehai-planner<br/>PRD · breakdown · sync diff"]:::engine
    tracker["ehai-tracker<br/>Tracker trait"]:::engine
    poc["ehai-poc<br/>worktrees · lineage"]:::engine
    env["ehai-env<br/>env files · deps · ports"]:::engine
    stack["ehai-stack<br/>stack + design tokens"]:::engine
    prefs["ehai-prefs<br/>learning store"]:::engine
    guard["ehai-guard<br/>data policy"]:::guard
    core["ehai-core<br/>config · paths · brand · migrations"]:::engine
  end

  subgraph outside["Outside world"]
    gh["GitHub Projects v2"]:::ext
    git["system git"]:::ext
    rel["GitHub Releases"]:::ext
  end

  subgraph disk["~/.ehai and repo .ehai/"]
    cfg[("config.toml")]:::store
    pf[("prefs/*.toml")]:::store
    wt[("worktrees/")]:::store
    ports[("ports.json")]:::store
    audit[("audit/*.jsonl")]:::store
  end

  pm --> cd & cc
  dev --> cc & oth
  cd & cc & oth -- "MCP stdio" --> mcp
  mcp --> planner & poc & prefs & guard
  planner --> tracker --> gh
  poc --> env & stack
  poc --> git
  env --> ports
  poc --> wt
  prefs --> pf
  guard --> audit
  cli --> core --> cfg
  cli -- "self-update" --> rel

  classDef user fill:#f97316,stroke:#c2410c,color:#fff
  classDef harness fill:#2563eb,stroke:#1e40af,color:#fff
  classDef engine fill:#7c3aed,stroke:#5b21b6,color:#fff
  classDef ext fill:#16a34a,stroke:#166534,color:#fff
  classDef store fill:#0891b2,stroke:#155e75,color:#fff
  classDef guard fill:#dc2626,stroke:#991b1b,color:#fff
```

### 4.2 Tech choices

| Layer | Choice | Why |
|---|---|---|
| Language | Rust (stable), Cargo workspace of 12 crates | One static binary, no runtime to install, fast startup per MCP session |
| AI interface | MCP via `rmcp`, stdio transport | Every major harness speaks MCP; stdio means no daemon to crash |
| CLI + wizard | `clap`, `cliclack`, `indicatif`, `ratatui` | Guided, polished terminal UX; a `Prompter` trait keeps it testable |
| Tracker | `reqwest` (rustls) + GraphQL for GitHub Projects v2 | No OpenSSL; Projects v2 is GraphQL-only |
| Git | shells out to system `git` | Worktrees behave exactly as the user's own git does |
| Config | TOML + `serde` + `schemars` | Human-editable, with a generated JSON schema |
| Updates | `self-replace` + SHA-256 + minisign | Verified, atomic swap with rollback |
| Tests | `insta`, `wiremock`, `proptest`, `assert_cmd`, `nextest` | Snapshots for every generated file, mocked HTTP, property-tested migrations |

### 4.3 Config layering

Narrower settings win:

```mermaid
flowchart LR
  d["Built-in defaults"]:::store --> g["~/.ehai/config.toml"]:::store --> r[".ehai.toml in repo"]:::store --> e["EHAI_* env vars"]:::harness --> f["CLI flags"]:::user --> out["Effective config"]:::engine
  classDef user fill:#f97316,stroke:#c2410c,color:#fff
  classDef harness fill:#2563eb,stroke:#1e40af,color:#fff
  classDef engine fill:#7c3aed,stroke:#5b21b6,color:#fff
  classDef store fill:#0891b2,stroke:#155e75,color:#fff
```

---

## 5. How a request flows

### 5.1 One tool call, end to end

The harness spawns `ehai mcp` per session. The engine loads your preferences, runs the tool, checks it with the guard, writes, and records what it learned.

```mermaid
sequenceDiagram
  autonumber
  actor U as 👤 User
  participant H as 🤖 AI harness
  participant M as ⚙️ ehai MCP server
  participant P as 💾 Prefs store
  participant G as 🛡️ Guard
  participant T as 🌐 Tracker

  U->>H: /ehai:sync
  H->>M: spawn "ehai mcp" (stdio)
  rect rgba(124, 58, 237, 0.15)
    H->>M: read resource ehai://prefs
    M->>P: load user + repo prefs
    P-->>M: compact preference block
    M-->>H: role, style, defaults
  end
  H->>M: call tool sync_plan
  rect rgba(220, 38, 38, 0.15)
    M->>G: scan titles, bodies, comments
    G-->>M: findings (kinds + counts only)
  end
  alt nothing found
    M->>T: diff local vs remote, write only changes
    T-->>M: created / updated items
    M->>P: observe interaction
    M-->>H: result + summary
    H-->>U: tables, diagram, next options
  else sensitive data found
    M-->>H: needs_input (redact / keep / cancel)
  end
```

### 5.2 The agent loop (`needs_input`)

ehai never guesses on a decision that belongs to the user. When a tool cannot continue on its own, it returns a structured question instead of failing. The harness shows it in its native multiple-choice UI and calls the same tool again with the answer.

```mermaid
flowchart TD
  start(["User asks for something"]):::user --> plan["🤖 Harness picks a skill<br/>and loads ehai://prefs"]:::harness
  plan --> tool["Call ehai MCP tool"]:::harness
  tool --> run["⚙️ Engine runs the step"]:::engine
  run --> check{"Can it finish<br/>without a decision?"}:::engine
  check -- "yes" --> guard{"🛡️ Guard passes?"}:::guard
  check -- "no" --> ni["Return needs_input<br/>question · options · retry_param"]:::engine
  guard -- "yes" --> write["Write to tracker / repo / worktree"]:::ext
  guard -- "no (warn / strict)" --> ni
  ni --> ask["❓ Harness asks with native<br/>multiple-choice UI"]:::harness
  ask --> ans(["User picks an option"]):::user
  ans -- "retry_param = choice" --> tool
  write --> learn["💾 Record observation in prefs"]:::store
  learn --> reply["Role-aware answer<br/>+ next options"]:::harness
  reply --> more{"More to do?"}:::user
  more -- "yes" --> plan
  more -- "no" --> stop(["Done"]):::user

  classDef user fill:#f97316,stroke:#c2410c,color:#fff
  classDef harness fill:#2563eb,stroke:#1e40af,color:#fff
  classDef engine fill:#7c3aed,stroke:#5b21b6,color:#fff
  classDef ext fill:#16a34a,stroke:#166534,color:#fff
  classDef store fill:#0891b2,stroke:#155e75,color:#fff
  classDef guard fill:#dc2626,stroke:#991b1b,color:#fff
```

Because the loop goes through MCP, it works the same in every harness. Clients that support MCP elicitation get the question directly; others render the `needs_input` result.

### 5.3 PM planning flow

```mermaid
sequenceDiagram
  autonumber
  actor PM as 👩‍💼 PM
  participant H as 🤖 Claude
  participant M as ⚙️ ehai
  participant T as 🌐 GitHub Projects

  PM->>H: /ehai:plan "let users export reports"
  loop Interview (max 4 questions per round)
    H->>PM: multiple-choice question
    PM-->>H: answer
  end
  H->>M: plan_explore (find gaps)
  H->>M: save_prd
  M-->>H: docs/prd/<slug>.md saved
  PM->>H: /ehai:breakdown
  H->>M: save_breakdown (epics, stories, tasks, Given/When/Then)
  H-->>PM: board preview + dependency graph
  PM->>H: Approve
  PM->>H: /ehai:sync
  H->>M: sync_plan
  M->>T: create / update only what changed (ehai-id footer)
  T-->>M: item links
  M-->>H: summary
  H-->>PM: "12 created, 0 duplicates" + link
```

Re-running `/ehai:sync` never duplicates items: each issue carries a stable `ehai-id`, and ehai diffs local against remote first.

### 5.4 POC pipeline

```mermaid
flowchart LR
  req["💡 Idea / task"]:::user --> und["1 · Understand<br/>clarify if needed"]:::harness
  und --> ana["2 · Analyze codebase<br/>stack · design tokens<br/>components · navigation"]:::engine
  ana --> place["3 · Placement<br/>where it lives, what it reuses"]:::engine
  place --> wt["4 · Isolated worktree<br/>ehai/poc/feature/poc-n"]:::store
  wt --> envp["Env pipeline<br/>(section 5.5)"]:::engine
  envp --> build["5 · Build with existing<br/>components + mock data"]:::harness
  build --> prev["6 · Preview<br/>dev server + screenshots"]:::ext
  prev --> reg["7 · Register in manifest<br/>lineage · feedback"]:::store
  reg --> fb{"Feedback?"}:::user
  fb -- "iterate" --> build
  fb -- "compose poc1 + poc2" --> wt
  fb -- "promote (explicit)" --> pr["Open PR to default branch"]:::ext
  fb -- "discard" --> arc["Archive tag<br/>remove worktree"]:::store

  classDef user fill:#f97316,stroke:#c2410c,color:#fff
  classDef harness fill:#2563eb,stroke:#1e40af,color:#fff
  classDef engine fill:#7c3aed,stroke:#5b21b6,color:#fff
  classDef ext fill:#16a34a,stroke:#166534,color:#fff
  classDef store fill:#0891b2,stroke:#155e75,color:#fff
```

POC lineage is a graph. `compose` resynthesizes parts of several POCs on a fresh branch from the base, instead of cherry-picking:

```mermaid
gitGraph
  commit id: "main"
  branch poc-1
  commit id: "poc1: list view"
  checkout main
  branch poc-2
  commit id: "poc2: card view"
  checkout poc-2
  commit id: "poc3: iterate poc2"
  checkout main
  branch poc-4
  commit id: "poc4: table from poc1 + header from poc3"
```

### 5.5 Worktree environment pipeline

A fresh worktree normally will not run. ehai makes it runnable:

```mermaid
flowchart LR
  a["1 · Discover<br/>gitignored .env*, local configs,<br/>keys from .env.example"]:::engine --> b["2 · Materialize<br/>copy or symlink"]:::engine
  b --> c["3 · Validate<br/>missing keys → ask"]:::harness
  c --> d["4 · Ports<br/>unique per worktree<br/>.env.ehai.local"]:::store
  d --> e["5 · Install deps<br/>pnpm / yarn / npm / bun<br/>go / uv / poetry / pip"]:::ext
  e --> f["6 · Health check<br/>smoke command"]:::engine
  f --> g["7 · Safety<br/>env files gitignored,<br/>values never leave"]:::guard
  g --> ok(["✅ Ready"]):::user

  classDef user fill:#f97316,stroke:#c2410c,color:#fff
  classDef harness fill:#2563eb,stroke:#1e40af,color:#fff
  classDef engine fill:#7c3aed,stroke:#5b21b6,color:#fff
  classDef ext fill:#16a34a,stroke:#166534,color:#fff
  classDef store fill:#0891b2,stroke:#155e75,color:#fff
  classDef guard fill:#dc2626,stroke:#991b1b,color:#fff
```

---

## 6. Preferences: how ehai learns

Each tool call can record an observation. Observations build confidence over time and become defaults; anything you state explicitly always wins.

```mermaid
flowchart TD
  act["👤 User acts<br/>likes a POC · picks an estimate scale<br/>answers the same question again"]:::user --> obs["⚙️ Tool records observation<br/>source = observed"]:::engine
  say["👤 User states a preference<br/>ehai prefs set · /ehai:prefs"]:::user --> exp["source = explicit<br/>never decays, never compacted"]:::engine
  obs --> store[("💾 Prefs store<br/>user.toml · project.toml")]:::store
  exp --> store
  store --> conf["Confidence<br/>↑ reinforced · ↓ contradicted · ↓ decays over time"]:::engine
  conf --> resolve{"Resolve per key"}:::engine
  resolve --> inject["ehai://prefs → compact<br/>'User preferences' block"]:::harness
  inject --> out["Better defaults,<br/>fewer questions,<br/>role-aware output"]:::ext
  out --> act

  classDef user fill:#f97316,stroke:#c2410c,color:#fff
  classDef harness fill:#2563eb,stroke:#1e40af,color:#fff
  classDef engine fill:#7c3aed,stroke:#5b21b6,color:#fff
  classDef ext fill:#16a34a,stroke:#166534,color:#fff
  classDef store fill:#0891b2,stroke:#155e75,color:#fff
```

**Resolution order** (first match wins):

```mermaid
flowchart LR
  r1["1 · Explicit repo<br/>team decision"]:::user --> r2["2 · Explicit<br/>user preference"]:::user --> r3["3 · Observed repo<br/>convention"]:::engine --> r4["4 · Observed<br/>user preference"]:::engine --> r5["5 · Defaults"]:::store
  classDef user fill:#f97316,stroke:#c2410c,color:#fff
  classDef engine fill:#7c3aed,stroke:#5b21b6,color:#fff
  classDef store fill:#0891b2,stroke:#155e75,color:#fff
```

Preferences describe behavior only (counts, booleans, labels), never sensitive data. Turn learning off with `prefs.learning = false`. More detail: [User preference agent](../reference/user-preference-agent.md).

---

## 7. Updates that never break

```mermaid
flowchart LR
  chk["Check channel<br/>alpha · beta · stable"]:::engine --> dl["Download to staging"]:::ext
  dl --> ver{"SHA-256 +<br/>minisign OK?"}:::guard
  ver -- "no" --> abort(["Abort, nothing changed"]):::guard
  ver -- "yes" --> keep["Keep current as ehai.prev"]:::store
  keep --> swap["Atomic swap"]:::engine
  swap --> doc{"doctor<br/>--post-update"}:::engine
  doc -- "pass" --> regen["Regenerate adapters"]:::harness --> ok(["✅ Updated"]):::user
  doc -- "fail" --> rb["Automatic rollback<br/>to ehai.prev"]:::guard

  classDef user fill:#f97316,stroke:#c2410c,color:#fff
  classDef harness fill:#2563eb,stroke:#1e40af,color:#fff
  classDef engine fill:#7c3aed,stroke:#5b21b6,color:#fff
  classDef ext fill:#16a34a,stroke:#166534,color:#fff
  classDef store fill:#0891b2,stroke:#155e75,color:#fff
  classDef guard fill:#dc2626,stroke:#991b1b,color:#fff
```

---

## Next

- [Quickstart](quickstart.md) — install and run `ehai init`
- [Use cases](use-cases.md) — PM, developer and POC walkthroughs
- [Data policy](compliance.md) — how the guard works
- [MCP tools](../reference/mcp-tools.md) — every tool the harness can call
