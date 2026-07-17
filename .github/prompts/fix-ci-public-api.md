# Task: Fix CI failures for the public-api sync branch

Make the branch pass the same gates that fail in GitHub Actions for this fork.

## Required green checks

1. `cargo fmt --all -- --check`
2. `cargo clippy --features sqlite,mysql,postgresql,enable_mimalloc,s3` (warnings are errors via `RUSTFLAGS=-Dwarnings`)
3. `typos` (spell check)

## Scope

- Prefer fixing code in `src/api/core/public.rs` and related public-api files.
- For German docs such as `TEST_PUBLIC_API.md`: either add the file to `.typos.toml` `extend-exclude`, or rewrite flagged words so `typos` passes. Do not invent unrelated docs.
- Do **not** create branches, commits, pushes, or PRs. CI commits after you edit files.
- Do **not** change product behavior of the public API except as required by Clippy/fmt.
- Do **not** modify `.github/workflows/**`.

## How to work

1. Read the failing command output provided in this prompt.
2. Apply minimal fixes until those commands would pass.
3. You may run `cargo fmt`, `cargo clippy ...`, and `typos` yourself to verify.

## Done when

Working tree changes are sufficient for fmt, clippy, and typos to succeed.
