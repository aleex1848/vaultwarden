# Task: Fix all CI / compile failures for the public-api sync branch

After merging upstream into this fork, make the branch compile and pass the CI gates.

## Required green checks

1. `cargo check --features sqlite` (must compile; type/API errors count)
2. `cargo fmt --all -- --check`
3. `cargo clippy --features sqlite,mysql,postgresql,enable_mimalloc,s3` (warnings are errors via `RUSTFLAGS=-Dwarnings`)
4. `typos` (spell check)

## Scope

- Prefer fixing code in `src/api/core/public.rs` and other fork/public-api files.
- When upstream changed function signatures, types, or call patterns (e.g. `log_event` now takes `EventType` instead of `i32`), **update the public-api code to match upstream**. Remove obsolete casts; pass the types the new API expects.
- Preserve public API HTTP behavior unless a compile/clippy fix requires a minimal change.
- For German docs such as `TEST_PUBLIC_API.md`: either add the file to `.typos.toml` `extend-exclude`, or rewrite flagged words so `typos` passes. Do not invent unrelated docs.
- Do **not** create branches, commits, pushes, or PRs. CI commits after you edit files.
- Do **not** modify `.github/workflows/**`.

## How to work

1. Read the failing command output provided in this prompt (especially `cargo check` / rustc errors).
2. Apply minimal fixes until those commands would pass — including adapting fork code to upstream API changes.
3. You may run `cargo check --features sqlite`, `cargo fmt`, `cargo clippy ...`, and `typos` yourself to verify.

## Done when

Working tree changes are sufficient for check, fmt, clippy, and typos to succeed.
