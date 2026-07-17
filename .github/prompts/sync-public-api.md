# Task: Resolve Vaultwarden upstream merge conflicts (public API fork)

You are resolving merge conflicts while syncing `dani-garcia/vaultwarden` (`upstream/main`) into this fork's `feature-public-api` branch.

## Goal

Keep the fork's **Organization Public API** additions working on top of the latest upstream Vaultwarden code.

## Scope (do only this)

1. Resolve all git conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`).
2. Prefer upstream signatures, types, imports, and call sites when Vaultwarden internals changed.
3. Preserve the fork's public API behavior and routes (groups, members, collections under `/public/...`), adapting code as needed so it compiles against upstream.
4. Touch only files required to finish the merge. Primary focus: `src/api/core/public.rs` (and directly related imports/modules if required).
5. Do **not** create commits, branches, tags, pushes, or PRs. Do **not** run `git merge` / `git rebase` / `git commit`. CI handles git after you edit files.
6. Do **not** add features, refactors, formatting-only churn, or dependency bumps unrelated to conflict resolution.

## How to work

1. Inspect `git status` and conflicted files.
2. Read both sides of each conflict and the surrounding upstream code for context.
3. Rewrite conflicted regions into correct, coherent Rust that matches current upstream APIs.
4. Remove every conflict marker.
5. Leave the working tree ready for `git add` on the resolved files.

## Done when

- No conflict markers remain in the tree.
- Resolved files look like valid Rust that preserves public API endpoints and aligns with upstream types/functions.
