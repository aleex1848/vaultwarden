#!/usr/bin/env bash
# Run fmt/clippy/typos; on failure, ask Cursor CLI to fix and retry.
set -euo pipefail

PROMPT_FILE="${PROMPT_FILE:-.github/prompts/fix-ci-public-api.md}"
CURSOR_MODEL="${CURSOR_MODEL:-composer-2.5}"
MAX_ROUNDS="${MAX_CI_FIX_ROUNDS:-5}"
CLIPPY_FEATURES="${CLIPPY_FEATURES:-sqlite,mysql,postgresql,enable_mimalloc,s3}"

log() { printf '==> %s\n' "$*"; }
die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

write_output() {
  local key="$1"
  local value="$2"
  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    {
      echo "${key}<<EOF"
      echo "${value}"
      echo "EOF"
    } >> "${GITHUB_OUTPUT}"
  fi
}

run_checks() {
  local log_file="$1"
  : > "${log_file}"
  local rc=0

  {
    echo "### cargo fmt"
    cargo fmt --all -- --check
  } >>"${log_file}" 2>&1 || rc=1

  {
    echo "### cargo clippy"
    cargo clippy --features "${CLIPPY_FEATURES}"
  } >>"${log_file}" 2>&1 || rc=1

  {
    echo "### typos"
    typos
  } >>"${log_file}" 2>&1 || rc=1

  return "${rc}"
}

commit_fixes_if_any() {
  local round="$1"
  if git diff --quiet && git diff --cached --quiet; then
    return 1
  fi
  git add -A
  # Never commit workflow changes via this helper.
  if git diff --cached --name-only | grep -q '^\.github/workflows/'; then
    git restore --staged .github/workflows/ || true
    git checkout -- .github/workflows/ 2>/dev/null || true
  fi
  if git diff --cached --quiet; then
    return 1
  fi
  git -c core.editor=true commit -m "chore(sync): auto-fix CI issues (round ${round})"
  return 0
}

require_cmd cargo
require_cmd typos
require_cmd agent

[[ -n "${CURSOR_API_KEY:-}" ]] || die "CURSOR_API_KEY is required to auto-fix CI failures"
[[ -f "${PROMPT_FILE}" ]] || die "prompt file not found: ${PROMPT_FILE}"

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

# Restore rust-toolchain.toml if an earlier step removed it for pinning.
if [[ ! -f rust-toolchain.toml ]] && git show HEAD:rust-toolchain.toml >/dev/null 2>&1; then
  git checkout HEAD -- rust-toolchain.toml
fi

rustup component add rustfmt clippy >/dev/null

FIXES_APPLIED="false"
LOG_FILE="$(mktemp)"
trap 'rm -f "${LOG_FILE}"' EXIT

if run_checks "${LOG_FILE}"; then
  log "fmt/clippy/typos already green"
  write_output "ci_fixed" "false"
  write_output "ci_ok" "true"
  exit 0
fi

log "CI checks failing — invoking Cursor to fix"
FIXES_APPLIED="true"

round=1
while (( round <= MAX_ROUNDS )); do
  log "Cursor CI-fix round ${round}/${MAX_ROUNDS}"
  FAIL_LOG="$(tail -n 200 "${LOG_FILE}")"
  PROMPT="$(cat "${PROMPT_FILE}")

---

Round: ${round}

Failing check output (tail):
\`\`\`
${FAIL_LOG}
\`\`\`

Fix the issues in the working tree now. Do not git commit/push.
"

  agent -p --force --output-format text --model "${CURSOR_MODEL}" "${PROMPT}"

  # Apply formatting deterministically when possible.
  cargo fmt --all || true

  if ! commit_fixes_if_any "${round}"; then
    log "Cursor made no committable changes in round ${round}"
  fi

  if run_checks "${LOG_FILE}"; then
    log "CI checks are green after round ${round}"
    write_output "ci_fixed" "${FIXES_APPLIED}"
    write_output "ci_ok" "true"
    exit 0
  fi

  ((round += 1))
done

log "CI checks still failing after ${MAX_ROUNDS} rounds"
tail -n 100 "${LOG_FILE}" >&2 || true
write_output "ci_fixed" "${FIXES_APPLIED}"
write_output "ci_ok" "false"
exit 1
