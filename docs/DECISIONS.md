# Decisions

ADR-style log. One entry per non-obvious decision. Newest last.

---

## ADR-0001 — Release repository
**Date:** 2026-11-17 · **Status:** accepted · **Amended:** 2026-11-18 (public, not private)

Releases are published to **`jagadeeshm007/ehai`**. (Changed from `jagadeeshm943/ehai` on 2026-11-17, before any release: `jagadeeshm007` is the account `gh` is signed into.) The owner/name appears in `Cargo.toml` (`workspace.package.repository`) and is used by `install/` and `ehai-update` (M8). It can move to an org later; keep it in those few places.

**Amendment (2026-11-18):** the spec's original "private GitHub repo" assumption is dropped — the user explicitly chose to keep the repo **public**. Nothing in the implementation actually required privacy: `install.sh`/`install.ps1` and `ehai-update` already treat `gh auth token`/`GITHUB_TOKEN` as optional (used only to avoid unauthenticated API rate limits), never as a hard requirement, so a public repo needs no code change — only the spec's wording (CLAUDE.md §11/§14) and this ADR needed correcting to stop describing a constraint that no longer applies.

## ADR-0002 — Create crates when a milestone needs them
**Date:** 2026-11-17 · **Status:** accepted

Spec §4 lists 12 crates. M0 creates only `ehai-core` and `ehai-cli`. Each remaining crate is added in the milestone that first puts code in it. Empty crates would add compile time, `Cargo.toml` noise and churn with nothing to test. The target layout in §4 is unchanged.

## ADR-0003 — Hand-rolled layered config instead of figment/config-rs
**Date:** 2026-11-17 · **Status:** accepted

- Each layer (global, repo, `--config`, env, CLI) is read as a raw `toml::Table`, deep-merged by a ~10-line recursive function (`ehai_core::config::deep_merge`), and deserialized once.
- This gives exact control over env mapping and migration-per-layer.
- It avoids a dependency whose provider model we'd fight.
- **Env mapping:** `EHAI_<SECTION>__<KEY>=value` maps to `section.key`. Only known top-level sections are mapped, so `EHAI_HOME`, `EHAI_NAME` and `EHAI_LIVE_GH` never leak into config. Values are parsed as TOML literals (`true`, `42`); anything else is a string.
- **Forward compatibility:** unknown keys are ignored on load, never rejected. Typed config is never written back. Only `migrate_file` rewrites a config file, and it works on raw tables, so unknown keys survive on disk.

## ADR-0004 — Prefer stdlib over small dependencies
**Date:** 2026-11-17 · **Status:** accepted

- File locking uses `std::fs::File::lock` (stable since 1.89) instead of `fs4`.
- The home directory comes from `std::env::home_dir` (behaviour fixed in 1.86) instead of `dirs`.
- Version metadata is a ~40-line `build.rs` (git sha, `SOURCE_DATE_EPOCH`-aware date, target) instead of `vergen`.
- Hence **MSRV = 1.89**. The toolchain is pinned to `stable` via `rust-toolchain.toml`.

## ADR-0005 — Doc-only changes: mirror workflow (`ci-skip.yml`)
**Date:** 2026-11-17 · **Status:** accepted

- `ci.yml` triggers only on code `paths:`.
- `ci-skip.yml` uses the identical list as `paths-ignore:` and defines jobs with the **same names** that just echo. Required status checks therefore pass on doc-only PRs, and doc-only commits run no real CI.
- We chose this over a `dorny/paths-filter` gate job because it literally meets "doc-only commits trigger no CI". A gate job would still start the CI workflow.
- **Caveat:** a PR that touches both docs and code runs both workflows. The real one is authoritative, so check names only go green when the real jobs pass.
- **Rule:** keep the two path lists in sync. Both files say so at the top.

## ADR-0006 — Terminal UI kit: cliclack behind a `Prompter` trait
**Date:** 2026-11-17 · **Status:** accepted

- All prompts go through `ui::Prompter`.
- `ClackPrompter` (cliclack) is used when stdin and stdout are TTYs and `--yes` is absent. `NO_COLOR` is honoured by `console`, the crate under cliclack. A NO_COLOR user on a TTY still gets interactive prompts, so it does not switch to plain mode.
- `PlainPrompter` is used for non-TTY or `--yes`. It accepts defaults and prints a plain transcript.
- The test fake is `PlainPrompter::scripted(answers)`, not a third implementation. Snapshots therefore show exactly what a CI user sees.
- `indicatif` and `ratatui` are deferred until a feature needs them (progress bars and the TUI views, M1+).

## ADR-0007 — Minimum supported OS versions
**Date:** 2026-11-17 · **Status:** accepted

| OS | Minimum | Notes |
|---|---|---|
| macOS | 12 | Release builds set `MACOSX_DEPLOYMENT_TARGET=12.0` |
| Windows | 10 1809 | Needed for ConPTY / modern terminal rendering |
| Linux | kernel 5.13 | Landlock available (sandbox §9A); musl static binaries run on any distro |

Older Linux kernels still run ehai. `ehai doctor` will report that Landlock is unavailable and fall back to path enforcement (M6).

## ADR-0008 — `EHAI_HOME` override
**Date:** 2026-11-17 · **Status:** accepted

`ehai_home()` honours `EHAI_HOME` before `~/.ehai`. Every test sets it to a tempdir, so tests never touch the real home directory. It also lets users relocate state.

## ADR-0009 — Release signing with a password-less minisign key
**Date:** 2026-11-17 · **Status:** accepted

- CI signs `SHA256SUMS` with minisign.
- The secret key (`minisign -G -W`, no password, so CI needs no TTY) is stored as the repo secret `MINISIGN_KEY`.
- The public key will be embedded in `ehai-update` (M8).
- The release job fails loudly if the secret is missing.

## ADR-0010 — Verified external versions
**Date:** 2026-11-17 · **Status:** accepted

Checked against crates.io and GitHub releases on 2026-11-17:
- Crates: clap 4.6, serde 1.0.229, toml 1.2, schemars 1.2, thiserror 2.0, tracing-subscriber 0.3.23, tracing-appender 0.2.5, cliclack 0.5.7, insta 1.49, proptest 1.12.
- GitHub Actions: checkout v7, upload-artifact v7, download-artifact v8, rust-cache v2, install-action v2, action-gh-release v3, cargo-deny-action v2.

Re-check before bumping.

## ADR-0011 — MCP server: rmcp 3.10 over stdio, panic guard per handler
**Date:** 2026-11-17 · **Status:** accepted

- **SDK:** `rmcp` 3.10, the official Rust SDK, with features `server`, `macros` and `transport-io`. Its default handler accepts every known protocol version (2024-11-05 … 2026-07-28), so older harnesses still negotiate.
- **Server lifetime:** `ehai mcp` serves stdio on a current-thread tokio runtime and exits cleanly when the client closes stdin. There is no daemon.
- **Panic guard:** every handler body runs through `ehai_mcp::guard`, which uses `tokio::spawn` and converts `JoinError::is_panic` into a structured error. One bad request can't kill the session.
- **Prefs resource URI:** `ehai://prefs` is fixed. The brand renames the MCP *server*, never the URIs, because internal identifiers stay `ehai` (spec §2.4).
- **Verified:** real Claude Code 2.1.179 shows `plugin:ehai:ehai … ✔ Connected` in `claude mcp list`.

## ADR-0012 — Claude Code adapter: a local marketplace with one plugin that bundles the MCP server
**Date:** 2026-11-17 · **Status:** accepted
**Sources:** code.claude.com/docs/en/plugins-reference, /plugin-marketplaces and /skills, fetched 2026-11-17. Checked against Claude Code CLI 2.1.179.

- **Plugin layout:**
  - `ehai` generates `~/.ehai/adapters/claude-code/.claude-plugin/marketplace.json`.
  - The marketplace (`<brand>-local`) lists a single plugin (`<brand>`).
  - The plugin holds `.mcp.json` (pointing at `<abs exe> mcp`) and `skills/<name>/SKILL.md`. Skills replace legacy `commands/`, as the docs recommend.
- **Install path:** `claude plugin marketplace add <dir>`, then `claude plugin install <brand>@<brand>-local --scope user`.
  - We chose this over `claude mcp add --scope user`: one install registers both the MCP server and the skills, and one `marketplace remove` reverses both.
  - It's idempotent: each step runs only if `--json` listings show it's missing.
- **Plugin versioning:** the docs suggest local-path plugins load in place, but 2.1.179 copies them into a **versioned cache**, and `plugin update` is a no-op when the version is unchanged. So `plugin.json` `version` = `<ehai version>+<FNV-1a hash of generated content>`. Changed content gets a new version, and install then runs `marketplace update` + `plugin update`. This was verified live: changing the role in `init` refreshed the installed skill.
- **Brand rename:** marketplaces whose path is our root but whose name differs are removed before the new one is added.
- **Reserved names:** `Brand::validate` rejects names Claude Code refuses for plugins (`claude`, `claude-*`, `anthropic-*`, `cc-plugin-*`).
- **Test strategy:**
  - Unit tests use a fake `claude` shell script, injected via `EHAI_CLAUDE_BIN`.
  - The live check used `CLAUDE_CONFIG_DIR=<tmp>` so the real user config was never touched.
  - `claude plugin validate` passes on both manifests.

## ADR-0013 — `ehai init` scope and behaviour
**Date:** 2026-11-17 · **Status:** accepted

- **Questions:** role, harnesses (pre-checked), compliance, sandbox, and repo-profile confirmation. That's ≤ 5, tested.
- **Tracker question:** moved to **M2**, where it can list the user's GitHub Projects live. Asking for the tracker kind alone now would be a wasted question.
- **No half-applied state:** nothing is written until all answers are in, so cancelling leaves nothing behind. Apply steps are idempotent, so rerunning `init` resumes.
- **Esc vs Ctrl-C:** cliclack reports both as the same `Interrupted`. A cancel opens "Go back / Quit": Esc then Enter goes back, and a second cancel quits with exit 130 and "Nothing was changed".
- **Retry:** a failed apply step offers "Retry?" defaulting to **no**, so `--yes` and non-TTY runs never loop.
- **Flags:** every question has one (`--role --harness --compliance --sandbox --no-repo`), and a flag skips its question.
- **Re-running:** existing config values become the defaults.
- **Chosen harnesses** are stored in `adapters.enabled`. `doctor` verifies, and `adapters regenerate` refreshes, only those.

## ADR-0014 — When repo onboarding runs
**Date:** 2026-11-17 · **Status:** accepted

- **Spec:** says onboarding runs "the first time ehai runs in a new repo (via any command)".
- **M1:** it runs from `ehai init` and `ehai repo init`.
  - `ehai mcp` must never prompt, because stdout is the protocol channel.
  - `doctor`, `config` and `adapters` are non-interactive.
  - The auto-trigger will hook into the interactive commands as they arrive (M3+).
- **What it writes:** `<repo>/.ehai/profile.toml` `[detected]` is rewritten on each scan. Other tables (team decisions) are preserved.
- **Local state:** a `.ehai/.gitignore` keeps it out of git without editing the repo's own `.gitignore`.
- **Secrets:** env files are recorded by **name only**, never read.

## ADR-0015 — External commands always have a timeout
**Date:** 2026-11-17 · **Status:** accepted

`ehai_core::process::output` spawns, drains stdout and stderr on threads (so a full pipe can't deadlock), and kills the process after a timeout. It covers `gh`, `git --version` and `claude` calls (120 s). Retries with backoff will be added for network calls in M2.

## ADR-0016 — `self install`, `connect`/`disconnect`, one-command `uninstall`
**Date:** 2026-11-17 · **Status:** accepted

- **`ehai self install`:**
  - Copies the running binary to `~/.ehai/bin/ehai` (copy-then-rename, so it's safe while running).
  - Adds the brand alias (`EHAI_NAME`/`brand.name`; a symlink on Unix).
  - Writes a marked `# >>> ehai >>>` PATH block into the login shell's rc: bash → `.bashrc`, zsh → `.zshrc`, fish → `conf.d/ehai.fish`, otherwise `.profile`. A `.ehai-backup` is taken before the first edit.
  - Re-points enabled adapters at the installed binary.
  - The M8 one-line installer downloads a release and calls this.
  - Windows: PATH is set by `install.ps1` (M8); `self install` prints the instruction.
- **Adapter binary path:** `adapter_ctx` prefers `~/.ehai/bin/ehai` over `current_exe()`. Harnesses then survive `cargo clean` or a deleted download folder. This was the bug where M1's `init` pointed Claude Code at `target/debug/ehai`.
- **`ehai connect <id>…` / `ehai disconnect <id>… | --all`:** friendly aliases for `adapters install/remove`. They also update `adapters.enabled`.
- **`ehai uninstall [--yes]`:**
  - Asks first, defaulting to *no*. A non-TTY run without `--yes` does nothing.
  - Unregisters **every** adapter, not only the enabled ones.
  - Strips the PATH blocks from all candidate rc files.
  - Deletes only entries ehai owns inside `~/.ehai`, and refuses if `EHAI_HOME` is the user's home or has no parent. Unknown files are kept and reported.
  - Skips file logging, so it doesn't recreate `~/.ehai/logs`.
  - Repo `.ehai/` profiles are left alone: they belong to the repo and may be committed.
- **Testing:** the rc-file location is passed in explicitly (`remove_files(home, user_home)`), so tests can never touch the developer's real dotfiles. A sandboxed-`HOME` lifecycle test proves that install → connect → disconnect → uninstall leaves no traces and restores `.bashrc` byte-for-byte.

## ADR-0017 — Tracker: GitHub Projects v2 via hand-written GraphQL
**Date:** 2026-11-17 · **Status:** accepted
**Sources:** GitHub's live GraphQL schema, introspected 2026-11-17 (`CreateProjectV2FieldInput`, `ProjectV2SingleSelectFieldOptionInput` with optional `id`, `UpdateProjectV2FieldInput`, `ProjectV2FieldValue`, `ProjectV2IterationFieldConfigurationInput`).

- **Trait:** `Tracker` uses native `async fn`-in-trait (`-> impl Future + Send`) with no `async-trait` dependency. Adapters are used generically. Linear and Jira are refused with a clear message until their adapters exist.
- **Hand-written GraphQL:** 15 documents in `github::q`, instead of `graphql_client` (schema download + codegen for that few operations).
- **Contract suite:**
  - One `contract()` runs against `MemoryTracker` (the reference) and `GithubProjects` + `FakeGithub`, a stateful wiremock GraphQL server behind the `fake` feature.
  - The fake models GitHub dropping a card's value when its option id disappears, so a destructive schema update would fail the tests.
- **Work items:**
  - Issues in the configured repo, added to the project, so they get comments and PR links.
  - Without a repo they're project draft items, which have no comments.
- **`ehai-id`:** an invisible `<!-- ehai-id: … -->` body footer.
  - The id charset is limited (`[A-Za-z0-9._-]`, no `--`), so it can't close the comment.
  - A footer followed by user text isn't treated as ours.
- **`ensure_schema` is never destructive:**
  - It creates missing Status/Priority/Size/Type single-selects and an Iteration field (a failure there is only a warning).
  - It adds missing options by sending existing options back **with their ids**, which keeps every card's value.
  - A same-named non-single-select field is reported, never touched.
- **Sync engine** (`sync::plan` is pure; `sync::sync` applies it):
  - Matches by `ehai-id` only.
  - Status is set only on create; afterwards the board owns it.
  - Orphans are reported, never deleted.
  - Per-item failures are collected.
  - Idempotence is property-tested.
- **Reliability:** 30 s timeout per request. Up to 3 attempts with backoff on network errors, 5xx, 429 and secondary rate limits (`Retry-After` honoured, capped at 30 s). 401 and `INSUFFICIENT_SCOPES` become actionable auth errors.
- **Auth:**
  - Order is `gh auth token`, then `GITHUB_TOKEN`, then `GH_TOKEN`. The token is read per run and never stored by ehai.
  - **OAuth device flow and keychain storage are deferred:** device flow needs ehai's own registered OAuth App client id, and borrowing another app's id would be impersonation. Both arrive together once the org registers an app.
  - Missing `project` scope is detected from `X-OAuth-Scopes`. ehai shows `gh auth refresh -s project` and never runs it (user decision).
  - Fine-grained tokens report no scopes, so ehai just tries.
- **`init` tracker question:**
  - It defaults to **Skip** unless a project is already connected, so `init --yes` never modifies a GitHub project nobody picked.
  - CI uses `--tracker-project N`, `--tracker-create TITLE` or `--no-tracker`.
  - It offers the repo's org projects when the user belongs to the org.
  - With this question, init asks at most 6 questions ("about 5", spec §14).
- **TLS:** reqwest 0.13's default rustls (aws-lc-rs + platform verifier, so corporate TLS proxies work). `deny.toml` now allows ISC, BSD-3-Clause, MIT-0 and CDLA-Permissive-2.0. No OpenSSL in the tree.
- **Tests stay offline:** CLI tests set `EHAI_NO_GH=1` and point `EHAI_GITHUB_API` at a fake or a dead port, so they never reach real GitHub. The live test runs only with `EHAI_LIVE_GH=1` and uses draft items, so no issues land in any repo. It deletes its throwaway project afterwards.

## ADR-0018 — Item type field is `Work type`; schema errors are per field
**Date:** 2026-11-17 · **Status:** accepted · **Amends:** ADR-0017

- **Found on a real project** (`jagadeeshm007` #2, "Team planning" template): GitHub refuses to create a field named `Type` ("Name cannot have a reserved value"); the name is kept for its built-in issue types.
- **Field name:** ehai's field is now **`Work type`**. An existing *single-select* `Type` field is still read and written, so teams that already have one keep using it.
- **Per-field errors:** `ensure_schema` now turns a failure on one field into a warning and carries on with the others. Auth errors still abort. Before this, one bad field stopped the run and left the rest unconfigured.
- **Template projects:** they already ship Status (Backlog/Ready/In progress/In review/Done), Priority, Size and Iteration. ehai matches option names case-insensitively, so it only added the missing `Todo` status and `Work type`.
- **Fake GitHub:** now models reserved field names, and can be made to fail a named field.
- **Broken pipes:** `ehai … | head` used to panic on the closed pipe. A panic hook plus a check in `main` now make it exit quietly with status 0.

## ADR-0019 — Updates omit unchanged fields (explicit `null` clears on GitHub)
**Date:** 2026-11-17 · **Status:** accepted

- **Bug:** the first full live run (`EHAI_LIVE_GH=1`) caught it. A title-only `update_item` sent `body: null`, and GitHub treats an explicit `null` as "clear", so the rename wiped the body and the `ehai-id` footer. An *omitted* variable leaves the argument unset.
- **Fix:** update mutations now include only the fields being changed.
- **Fake GitHub:** now applies the same `null` vs omitted rule. The contract suite renames by title alone, and it **fails on the old code** and passes on the fix.
- **Also confirmed live:**
  - `updateProjectV2Field` with existing option ids keeps a card's value (In Progress survived adding Backlog and In Review).
  - `Work type`/Priority/Size/Iteration creation works.
  - The throwaway project is deleted.

## ADR-0020 — Compliance guard: confidence levels, kinds-only reporting
**Date:** 2026-11-17 · **Status:** accepted

- **Built-in detectors** (`ehai-guard`, regex):
  - SSN (with area/group/serial validity checks)
  - MRN / medical record number
  - DOB (keyword plus date)
  - insurance/member/policy id, email, phone, street address
  - a **name next to a medical term** heuristic
  - Org-specific patterns come from `compliance.patterns`.
- **Confidence levels:**
  - Structured identifiers are **High**.
  - The name heuristic is **Low**: it's surfaced but never auto-redacted, because rewriting phrases like "Book Appointment" in strict mode would corrupt PRDs. A stop-list of UI/story words keeps "Given the patient… Book Appointment" from being flagged.
- **Modes:**
  - `warn` returns findings and lets the user choose redact / keep / cancel.
  - `strict` redacts High automatically and reports Low.
  - `off` skips checks.
- **Values are never reported:** a finding has a kind and a byte span only. Questions, tool output and the audit log name kinds ("mrn ×1, email ×1").
- **Audit log:** monthly JSONL files under ehai's home (`audit` dir), one locked append per write. It records command, target, location, kinds and action.
- **Tests:** a property test checks that generated SSN/MRN/email/DOB/phone/member-id samples in random prose are always found and removed by redaction.

## ADR-0021 — PM flow: the harness writes, the engine shapes, checks and syncs
**Date:** 2026-11-17 · **Status:** accepted

- **Division of labour:** interviewing, writing and classifying are the harness model's job. `ehai-planner` gives the output a fixed shape (`Prd`, `Breakdown` → `Epic` → `Story` → `Task`, Given/When/Then), validates it and renders it.
  - PRD validation: metrics need a number, flows must be Mermaid.
  - Breakdown validation: ids unique and valid, dependencies known, no cycles, every story has criteria.
  - Outputs: Markdown tables, Mermaid graphs, a self-contained HTML report.
  - It also produces `sync::Desired` items keyed `<prd-slug>.<id>` (≤ 64 chars).
- **Storage:** plans live in `~/.ehai/plans/<slug>/` (so Claude desktop works without a repo). PRDs are also written to `docs/prd/<slug>.md` when a repo is known.
- **Tools:** `save_prd`, `get_plan`, `list_plans`, `save_breakdown`, `sync_plan` (preview by default, `apply: true` to write), `status_report` (optional HTML) and `triage` (duplicate check by content-word Jaccard ≥ 0.35), plus `guard_check` and `ping`.
- **`needs_input`:** questions use `{status:"needs_input", question, header, options[{label,value,description}], multi_select, retry_param}`. The harness asks through its own question UI and calls the same tool again with `retry_param` set to the chosen value. This works in every MCP client; MCP elicitation can be added later without changing tools.
- **Schema before sync:** `sync_plan` runs `ensure_schema` before writing. The PM-flow test caught that a project without ehai's fields made every later sync show phantom "updates", because the field values couldn't be set.
- **Status reports:**
  - Epics are excluded from counts; their progress is derived from their stories and tasks.
  - "Blocked" means started while a dependency is unfinished.
  - "Stale" means In Progress/In Review with no change in 14 days (ISO timestamps compare as strings).
  - "Risk" means a P0 that hasn't started.
  - Linked-PR counts are left for the dev flow (M6).

## ADR-0022 — Claude desktop: config entry + MCP prompts, not skills
**Date:** 2026-11-17 · **Status:** accepted
**Sources (2026-11-17):** support.claude.com "Using Skills in Claude" (custom skills are ZIP uploads in Settings only), Claude desktop MCP setup guides, and the file on this Linux machine (`~/.config/Claude/claude_desktop_config.json`).

- **Config entry:** the adapter edits only `mcpServers.<brand>`.
  - Key order is preserved (`serde_json` `preserve_order`; this adds the Zlib-licensed `foldhash`).
  - A `.json.ehai-backup` is taken before the first edit.
  - A file that isn't valid JSON is refused, never overwritten.
  - The lock file lives in `~/.ehai/adapters/`, never in Claude's directory.
  - The desktop app needs a full quit and relaunch afterwards; the install message says so.
- **Why prompts:** the PM flows reach desktop as **MCP prompts** served by the same server, rendered from the same templates as the Claude Code skills. That gives one source and zero manual steps. ZIP skill export can be added if the team wants skills there too.

## ADR-0023 — Fixes from the first live end-to-end run (aisdlc-demo → project #2)
**Date:** 2026-11-17 · **Status:** accepted

The run drove the installed binary's MCP server against real GitHub through steps 0–10. It found:

1. **`tracker set` saved before verifying.** A mistyped owner was written to config, then failed. Now the field setup (which proves the project exists) runs first, and settings are saved only after it succeeds. A test for a missing project checks that config is untouched.
2. **Plain TypeScript package without framework dependencies → "no apps".** Left as is for now; app detection is revisited with stack work in M5.
3. **A rename rewrote every related issue,** because descriptions quoted other items by title. Descriptions now use `[[ehai-ref:<id>|<title>]]` tokens, resolved to `#<number>` once issues exist (GitHub renders the live title), or to the title for draft items.
   - Sync applies in two passes when it creates items: pass 1 creates them, pass 2 fills in the links. A re-sync is therefore a no-op.
   - Verified live: renaming one story updated 1 issue and left 3 untouched.
   - The first sync on an older board migrates title references to `#n` once.
4. **Enum values serialized as Rust names** (`Xs`, `InProgress`), while skills, people and the board say `XS` / `In Progress`. An agent sending "XS" was rejected. Status, Priority, Size and Work type now serialize as board labels, parse case- and space-insensitively (old saved plans still load), and advertise their labels in tool schemas.

## ADR-0024 — Roles and team: admin status gates at read time, never gates actions
**Date:** 2026-11-17 · **Status:** accepted
**Plan:** `docs/plans/roles-and-team.md` (approved)

- **Admin signal:** `ProjectV2.viewerCanClose` is the only field GitHub exposes that reliably means "this viewer administers this project" — confirmed by live GraphQL introspection against the user's own account. It's checked once, when a project is connected (`tracker set`/`init`), and the result is stored as `access.project_admin` + `access.checked` (a date, for `role`'s explanation text).
- **Gate at the read site, not the write site:** `Config::effective_role()` is the single place role gating happens — `access.project_admin == Some(true)` unlocks the stored `role`/`EHAI_ROLE` override; anything else is `dev`. This means a stored `role` or env override is never dangerous to keep around: nothing needs to scrub or force-reset it when admin status changes. Losing admin on a refresh (someone else now owns the project) silently drops back to `dev` with no special-case code.
- **No action is permission-gated.** The user was explicit: every ehai action — assign, comment, plan, sync, triage — is available to every role. Role only changes tone, defaults and what a skill leads with (e.g. `status`'s pm vs. dev framing). The only thing actually gated is `role set` itself: refused for non-admins, with a clear explanation, because letting a non-admin grant themselves `pm` would defeat the whole point of tying role to real project control.
- **Team = assignable ∪ org members**, merged by login in one combined GraphQL query (`@include(if: $hasRepo)` makes the repo-assignable-users branch conditional). This is the user's explicit choice over "assignable only" or "org members only" — a repo collaborator who isn't an org member, and an org member who isn't yet a repo collaborator, both need to show up so `assign_task`/`comment_task` can resolve them.
- **Cache policy:** a day old by default (`ehai team --refresh` forces it sooner), stored at `~/.ehai/cache/team/<owner>-<number>.json`. A failed refresh falls back to the stale cache rather than erroring, since a day-old roster is almost always still useful and a transient GitHub/network failure shouldn't block `assign_task`.
- **Draft issues can't be assigned additively.** GitHub's `Assignable` union (which `addAssigneesToAssignable`/`removeAssigneesToAssignable` operate on) doesn't include `DraftIssue` — only `updateProjectV2DraftIssue` touches its assignees, and that mutation replaces the whole list. `GithubProjects::assign`/`unassign` branch on item kind: Issues use the additive mutations directly; drafts read the current assignee ids first, then write back the merged/filtered list.
- **Assignees are set-on-create-only**, mirroring the existing status rule: `sync::Desired.assignees` only take effect when `sync` creates an item. A later sync with different planned assignees never overwrites a manual reassignment someone made on the board.
- **Verified live** two ways: against the real `jagadeeshm007/aisdlc-demo` repo + project #2 (re-running `tracker set` correctly detected and stored admin status, `ehai team` returned the one real assignable user), and against a disposable throwaway project created and deleted by the existing `EHAI_LIVE_GH=1` live test (`crates/ehai-tracker/tests/live.rs`), extended to cover `project_admin`, `team::fetch` and an assign/unassign round-trip.

## ADR-0025 — Env provisioning: "not tracked" over "gitignored", skip over fail
**Date:** 2026-11-17 · **Status:** accepted

- **Discovery test is "not tracked by git", not literally "gitignored".** The spec's step 1 says gitignored config; the implementation instead keeps any pattern-matching file that `git ls-files` doesn't know about. A freshly created `.env` that hasn't been committed yet is exactly as safe and useful to copy as one `.gitignore` already excludes, and checking "not tracked" needs no `.gitignore` parsing (git already resolves nested, negated and repo-root-relative ignore rules correctly; re-implementing that would be the bug-prone path). The one case this changes from a literal reading: a tracked file that happens to match a pattern is correctly left alone either way, since `is_tracked` also gates `materialize`.
- **Patterns are a hand-rolled glob-to-regex translator** (`*` → `.*`, everything else escaped), not the `glob` crate: the pattern set is fixed and small (a dozen built-ins plus user globs), and `regex` is already a workspace dependency. Case-insensitive by construction, since `google-services.json`-style filenames vary in practice.
- **Install failures never fail the sync.** A missing package manager, a missing lockfile, or the install command itself failing all land in `Report.skipped`, not an `Err`. Only a step that makes every later step meaningless (discovery, materialising, or port allocation) can fail the whole call — installing dependencies for one app failing must not stop ports from being written or the smoke check from running for the others. Caught by a real bug in review: the first cut returned `Option<String>` from the install helper and pushed *every* `Some(..)` — including "tool not found" — into `installed`; fixed with a 4-variant `InstallOutcome` enum and a test that deliberately picks a package manager (`yarn`) absent from the dev machine.
- **Ports are keyed by the worktree's own canonical path**, not a caller-supplied id, so `ehai env sync` run twice on the same directory is idempotent without the CLI/MCP layer having to invent or persist an id. `ehai-poc` (M5) can pass its own worktree path the same way once it exists.
- **The source checkout to copy from is found via `git rev-parse --git-common-dir`**, not a parameter the user must supply: every worktree of a repo shares one `.git` dir, and its parent (when the name is literally `.git`, i.e. not a bare/linked worktree's own gitdir) is the main checkout. Falls back to treating the target as its own source (a safe no-op) when that doesn't resolve, e.g. the target isn't a worktree at all yet.
- **Fixture coverage is synthetic, not real installs.** Spec §13 names fixture repos (RN Expo, bare RN, Vite React, Next.js, Go API, FastAPI, pnpm monorepo); each is a handful of files (`package.json`/`go.mod`/`pyproject.toml`, a `.env`/`.env.example` pair) sufficient for `ehai-stack` detection and `ehai-env` discovery to fire deterministically, with `install_deps: false` and a trivial `exit 0` smoke command so "ready" doesn't depend on real toolchains or network access being available wherever the suite runs. The install step itself is exercised separately against whatever package managers are actually absent on the dev machine (`yarn`, verified via `which`), not simulated.
- **Coverage wasn't measured with `cargo-llvm-cov`** (not installed here) — left as a gap in `PROGRESS.md` rather than claimed.

## ADR-0026 — M7 harness adapter config paths (verified 2026-11-17)
**Date:** 2026-11-17 · **Status:** accepted

Config paths and formats verified against each harness's official docs before implementation. Sources and locations:

| Harness | Config key | Config path | Source (verified 2026-11-17) |
|---------|-----------|-------------|-------------------------------|
| Codex CLI | `mcp_servers.<brand>` TOML section | `~/.codex/config.toml` | github.com/openai/codex README and source |
| Gemini CLI | `mcpServers.<brand>` JSON | `~/.gemini/settings.json` | google.com/gemini/cli docs |
| Cursor | `mcpServers.<brand>` JSON | `~/.cursor/mcp.json` | cursor.com/docs/cmdk/mcp |
| OpenCode | `mcpServers.<brand>` JSON | `~/.config/opencode/config.json` | opencode.ai docs / GitHub README |
| VS Code | `servers.<brand>` JSON (note: `servers`, not `mcpServers`, + `type: "stdio"` required) | `~/.config/Code/User/mcp.json` (Linux), `~/Library/Application Support/Code/User/mcp.json` (macOS), `%APPDATA%\Code\User\mcp.json` (Windows) | code.visualstudio.com/docs/copilot/chat/mcp-servers |
| Windsurf | `mcpServers.<brand>` JSON | `~/.codeium/windsurf/mcp_server_config.json` | codeium.com/windsurf/mcp and Windsurf settings |
| AGENTS.md | marked block `<!-- >>> ehai >>> -->` | `~/AGENTS.md` | openai.com/index/agents-sdk (universal harness convention) |

**Supplementary file dirs:**

| Harness | File type | Dir |
|---------|-----------|-----|
| Codex CLI | prompt `.md` | `~/.codex/prompts/` |
| Gemini CLI | command `.toml` | `~/.gemini/commands/` |
| Cursor | rule `.mdc` (with frontmatter) | `~/.cursor/rules/` |
| OpenCode | prompt `.md` | `~/.config/opencode/prompts/` |
| VS Code | prompt `.prompt.md` | `<user-data>/prompts/` |
| Windsurf | workflow `.md` | `~/.codeium/windsurf/workflows/` |

**Key differences from other adapters:**
- VS Code is the only harness that uses `servers` (not `mcpServers`) and requires `"type": "stdio"` in every entry.
- Codex uses TOML with a marked-block strategy (`# >>> ehai >>>` / `# <<< ehai <<<`) rather than JSON key patching.
- AGENTS.md uses HTML comment markers (`<!-- >>> ehai >>> -->`) to survive markdown rendering.
- All five JSON adapters share `json_mcp.rs` (a 90-line module) to avoid duplicating the read/backup/patch/write pattern.

## ADR-0027 — Closing the M6 sandbox gap; keeping ADR-0022's desktop decision

**Date:** 2026-11-17 · **Status:** accepted

- **Per-worktree Claude Code sandbox settings** (spec §9A's last bullet, flagged as a known gap at the end of M6): every new POC worktree gets a `.claude/settings.local.json` written by `ehai-poc::write_worktree_sandbox_settings` — edits and local dev commands allowed, `git push`/`git reset --hard`/checking out the default branch/`rm -rf` outside the worktree denied. Best-effort and idempotent: a failure to write it never aborts worktree creation, and a second call on the same worktree (e.g. `poc iterate`) leaves an existing file alone rather than re-writing it.
- **The Claude desktop skills-ZIP idea was tried and reverted before landing.** A half-finished attempt (doc comment, `zip`/`ZipWriter` imports, an unused `PM_SKILLS` const — no actual generation code) was found uncommitted in `claude_desktop.rs`, contradicting the already-settled ADR-0022 ("MCP prompts are sufficient... No ZIP skill package needed", still the live line in `PROGRESS.md`'s Open Questions). Removed rather than finished: ADR-0022 already covers why prompts are the zero-step route, and nothing had changed to reopen that decision. The `zip` dependency it pulled into `ehai-adapters` was removed with it; `zip` itself stays a real (non-dev) workspace dependency because `ehai migration export/import` (below) genuinely needs it.
- **`ehai migration export/import`** bundles `config.toml`, `prefs/user.toml` and the local tracker's `tracker.json` into one ZIP, with a `schema_version`-tagged `metadata.json` so a future ehai can refuse an export it doesn't understand instead of silently corrupting config. Harness adapter files are deliberately excluded — they're machine-specific (absolute paths to the binary, OS-specific config locations) and regenerate cheaply via `ehai adapters regenerate`, so bundling them would just be stale data waiting to cause confusion on the new machine.
- **`plan_explore` and the `ehai-prefs` observation hook are additions beyond the original milestone list**, not spec requirements — a cheap PRD-quality pass and some lightweight behavioural signal capture, both already wired into `save_prd`/`poc_new`. Recorded here rather than silently absorbed into "M8" so a later contributor doesn't go looking for them in the spec and come up empty.
- **Found and fixed in review:** `explore::find_gaps`'s single-flow error-handling check had a vacuous first clause (`mermaid.contains(|c| matches!(c, 'e'))` — true for almost any English sentence, including ones with no actual error handling), which made the real check (`contains("error")`/`"fail"`/`"fallback"`) do nothing whenever the flow text happened to lack the letter 'e' — the one case anyone would've noticed, since it's rare. Neither branch had a test; both do now.

## ADR-0028 — Self-update: untestable by design where it matters, so test around it

**Date:** 2026-11-18 · **Status:** accepted

- **`self-replace` over hand-rolled rename logic.** The spec names it explicitly (§3). Unix can usually just rename over a running executable; Windows can't, and `self-replace` already contains the (nontrivial, documented in its own crate docs) workaround — copy-and-relaunch-to-delete. Writing that ourselves would be reinventing a correctness-critical wheel for one platform we can't even test from this Linux sandbox.
- **`self_replace::self_replace(new_path)` always targets `std::env::current_exe()` of the calling process — never a path you hand it.** This is the single most important fact shaping the whole crate's test strategy: it means `apply`/`swap_in`/`rollback` can **never** be called from an automated test running as part of the normal `cargo test`/`ehai` binary, because doing so would replace the actual test binary (or, if run via `assert_cmd` against the real `ehai` CLI, the shared `target/debug/ehai` build artifact) out from under the test harness. `stage()` (network, checksum, signature, archive extraction) is therefore split out as the fully unit/integration-tested half; `swap_in`/`rollback` are thin, deliberately untested wrappers, documented as such in the module doc so a future contributor doesn't "helpfully" add a test that corrupts their own build.
- **The real swap/rollback mechanics are still verified**, not just trusted on faith: a disposable helper binary (`src/bin/update_test_harness.rs`, `swap`/`rollback`/`version` subcommands) is copied into a tempdir and spawned as its own subprocess. Its `current_exe()` then correctly points at the tempdir copy, so the test can genuinely exercise `self_replace` — backup written, swap completes, the result is still a runnable executable, rollback restores the exact original bytes — without the risk applying to anything but a disposable file. This is the "update test simulates a failed post-update doctor, and rollback restores the previous binary" item from milestones.md M8.
- **Verifying a signature needs a signature — minisign-verify is verify-only (no signing API), so round-trip tests use a real, throwaway keypair** generated with the actual `minisign` CLI (happened to be installed in this environment), not a fabricated/hand-written base64 blob pretending to be a valid signature. A fabricated fixture would either fail to parse (useless test) or — worse — silently pass because nothing was actually checked, which is indistinguishable from a real bug at review time. The committed fixtures (`verify.rs`'s `TEST_PUBLIC_KEY`/`TEST_SUMS`/`TEST_SIG`, and `tests/update_flow.rs`'s equivalents) are this throwaway key's output, never the production `MINISIGN_PUBLIC_KEY`.
- **`stage()` takes the public key as a parameter instead of reading `ehai_core::release::MINISIGN_PUBLIC_KEY` directly.** The constant is still the right default for every real caller (`ehai update`, `install.sh`'s Rust-side equivalent), but hardcoding it inside `stage()` would make the function untestable without the real production private key, which no test environment has or should have.
- **The post-update check spawns the new binary as a fresh subprocess** (`<exe> doctor --post-update`), not an in-process call. `apply()`'s file swap only changes what's on disk — the process that performed the swap is still executing the *old* code until it exits, so verifying "does the new binary actually work" requires actually starting it.
- **The daily background check is a bounded-timeout, best-effort courtesy, not a feature with its own retry/backoff logic.** Any failure — offline, DNS, a slow API, rate limiting — is silently swallowed and the timestamp is still recorded, so a flaky network never turns into a daily nag or a slow command; the user finds out for real the next time they run `ehai update` on purpose.
- **The embedded `MINISIGN_PUBLIC_KEY` turned out to already be the real one.** The keypair generated during this session (`~/.ehai/minisign.pub`/`.key`) is what the user uploaded as the `MINISIGN_KEY` GitHub Actions secret — same key, so the constant needed no change, only the "this is a placeholder" caveat removing once confirmed.

## ADR-0029 — First push: cargo-deny findings from the real CI run

**Date:** 2026-11-18 · **Status:** accepted

The first real push surfaced three `cargo-deny` failures `cargo build`/`test`/`clippy` don't check, since none of them are compile errors:

- **`zip` 2.6.1 carried two real vulnerabilities** (RUSTSEC-2026-0545: AES authentication-tag bypass; RUSTSEC-2026-0548: `ZipStreamReader` can extract under local-header names that differ from the central directory). Neither affects how `ehai` actually uses the crate (no AES-encrypted archives, no `ZipStreamReader`), but a vulnerable dependency version is still a vulnerable dependency version. Fixed by upgrading to `zip` 9.x, which also meant adapting `archive.rs` to `ZipFile::name()` now returning `Result<Cow<str>, ZipError>` instead of `&str` (the crate made entry names fallible — a malformed name can't panic the caller anymore).
- **That upgrade's default features pulled in `bzip2`/`zstd`/`lzma` support** `ehai` never uses (only plain DEFLATE, for both the migration export ZIP and release asset extraction), and with them two new transitive licenses (`bzip2-1.0.6`, `BSD-2-Clause`) that weren't yet in `deny.toml`'s allow-list. Rather than widening the allow-list for license surface we don't need, `zip` is now pulled with `default-features = false, features = ["deflate"]` — smaller dependency tree, no new licenses, same functionality.
- **`chrono` is unmaintained (RUSTSEC-2026-0543), with "no safe upgrade available" per the advisory itself** — it's transitive, via `rmcp` (the MCP SDK) and `schemars` (config JSON schema generation), neither of which we can drop. Added to `deny.toml`'s `[advisories] ignore` list with a comment explaining why, which is the mechanism `cargo-deny` provides for exactly this situation (a real, accepted finding with no available fix, rather than a false positive).
- None of this was caught by the earlier `cargo build`/`test`/`clippy --workspace` runs in this session, because `cargo-deny` checks advisory databases and license metadata that plain compilation doesn't touch — the gap between "builds and passes tests" and "passes the full CI matrix" is exactly what the first real push is for.

The second push then surfaced two more, this time from the 3-OS test matrix — this environment is Linux x86_64 only, so cross-platform bugs in tests (not production code) had nowhere to be caught before a real multi-OS run:

- **`ehai-env`'s install-skip test assumed `yarn` specifically was absent from PATH** (true on this dev machine, false on GitHub's hosted runners, which ship Node.js with yarn). Rather than mutate the real `PATH` env var — which would race every other test in the same binary under `cargo test`'s default multi-threaded, single-process execution (confirmed by reproducing the exact failure locally) — `ehai-env::which()` gained an `EHAI_ENV_PATH` override that only it reads, so the test can force "nothing found" deterministically without touching the `PATH` that `git`, `sh`, and everything else in the same process still needs. (Also confirmed in the same investigation: `env::set_var` is *not* an unsafe fn under this workspace's `edition = "2021"` — that's an edition-2024-only change — so the earlier instinct to wrap it in `unsafe{}`, which this workspace's `unsafe_code = "forbid"` lint then rejected, was solving a problem that didn't exist here.)
- **`ehai-update`'s fixture tests hardcoded a single target** (`x86_64-unknown-linux-musl`), so they failed with `NoAsset` on GitHub's `macos-latest` runner, which is Apple Silicon (`aarch64-apple-darwin`). Fixed by signing one `SHA256SUMS` fixture covering all five targets in the real release matrix (same archive bytes, same hash, one line per target) with the same throwaway test key, so the suite verifies correctly regardless of which OS/arch actually runs it.
- **The `snapshots` CI job's `cargo insta test` invocation passed `--locked` where `cargo insta test` itself doesn't accept it** (it only forwards unrecognised flags placed after `--` to the underlying `cargo test`), and its doctest runner rejected `--locked` even there. Dropped it from this one job — lockfile integrity is already enforced by `clippy`, `test` and `deny`, all of which use it correctly.
- **The real root cause of the Windows snapshot mismatch wasn't path rendering — the repo had no `.gitattributes`.** `claude_code.rs`'s adapter embeds a content hash (`content_hash(&files)`, hex-encoded into the generated plugin's `version` field) computed over the skill `.md` templates, which are pulled in at compile time via `include_str!`. Without `.gitattributes`, GitHub's `windows-latest` runner's default `core.autocrlf=true` checks those templates out with CRLF line endings instead of the LF they're committed with, so the hash — and therefore the generated `version` string's literal text — genuinely differed from the Unix-recorded snapshot. The backslash/CRLF normalization added to the test earlier was real and worth keeping (belt-and-suspenders for path rendering), but it couldn't fix *this*, since differing hash digits aren't a formatting difference to normalize away. Root-caused by adding `.gitattributes` (`* text=auto eol=lf`) so every platform checks out byte-identical content, which is the actual fix — "make the build inputs identical everywhere" beats "patch the test that happened to notice."
- **`ehai init` now runs the equivalent of `ehai self install` as its first apply step**, unconditionally and idempotently, rather than leaving "install to `~/.ehai/bin` and add to PATH" as a separate manual step the one-line installer happened to chain but a from-source build wouldn't. Two reasons: (1) spec §2.10, "do it for the user" — PATH setup is exactly the kind of thing that shouldn't need a second command; (2) harness adapters are wired via `adapter_ctx`'s "prefer the installed binary" rule, so running `init` from a build directory or a download folder without this step would silently point every harness's MCP config at a location that disappears on `cargo clean` or once the download is deleted.
