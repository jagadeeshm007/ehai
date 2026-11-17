## Summary

<!-- One paragraph: what does this change and why? Link the issue: Fixes #123 -->

## Changes

<!-- Bullet list of the specific files or systems changed. -->

-

## Test plan

- [ ] `cargo nextest run --workspace` — all tests pass
- [ ] `cargo clippy --all-targets -- -D warnings` — clean
- [ ] `cargo fmt --check` — clean
- [ ] `cargo deny check` — no new advisories
- [ ] Snapshots regenerated and reviewed (`cargo insta review`) if template text changed
- [ ] New behaviour has a test that fails without this change

<!-- Add manual test steps if relevant (e.g. "ran ehai init on macOS 14 and confirmed X"). -->

## Breaking changes

<!-- Does this change any MCP tool parameter names, response keys, config keys, or CLI flags? -->

- [ ] No breaking changes
- [ ] Yes — described below

<!-- If yes, describe what changes and what callers need to update. -->

## Notes for reviewer

<!-- Anything non-obvious about the approach, trade-offs made, or areas to focus on. -->
