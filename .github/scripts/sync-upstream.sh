#!/usr/bin/env bash
# Merge upstream/main into feature-public-api, optionally resolving conflicts with Cursor CLI.
set -euo pipefail

FEATURE_BRANCH="${FEATURE_BRANCH:-feature-public-api}"
SYNC_BRANCH="${SYNC_BRANCH:-sync/upstream-public-api}"
UPSTREAM_URL="${UPSTREAM_URL:-https://github.com/dani-garcia/vaultwarden.git}"
UPSTREAM_REF="${UPSTREAM_REF:-main}"
PROMPT_FILE="${PROMPT_FILE:-.github/prompts/sync-public-api.md}"
CURSOR_MODEL="${CURSOR_MODEL:-composer-2.5}"
MAX_CURSOR_ROUNDS="${MAX_CURSOR_ROUNDS:-8}"

log() { printf '==> %s\n' "$*"; }
die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

conflicted_files() {
  git diff --name-only --diff-filter=U
}

has_conflicts() {
  [[ -n "$(conflicted_files)" ]]
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

resolve_with_cursor() {
  require_cmd agent
  [[ -n "${CURSOR_API_KEY:-}" ]] || die "merge conflicts need CURSOR_API_KEY secret, but it is not set"
  [[ -f "${PROMPT_FILE}" ]] || die "prompt file not found: ${PROMPT_FILE}"

  local round=1
  while has_conflicts; do
    if (( round > MAX_CURSOR_ROUNDS )); then
      die "still have merge conflicts after ${MAX_CURSOR_ROUNDS} Cursor rounds"
    fi

    log "Cursor conflict resolution round ${round}/${MAX_CURSOR_ROUNDS}"
    mapfile -t files < <(conflicted_files)
    printf 'Conflicted files:\n'
    printf '  - %s\n' "${files[@]}"

    local prompt
    prompt="$(cat "${PROMPT_FILE}")

---

Round: ${round}
Conflicted files:
$(printf '%s\n' "${files[@]}")

git status:
$(git status --short)

Resolve the conflicts in the working tree now. Do not run git commit/merge/rebase.
"

    agent -p --force --output-format text --model "${CURSOR_MODEL}" "${prompt}"

    # Stage any formerly conflicted paths that no longer have markers.
    local f
    for f in "${files[@]}"; do
      if [[ -f "${f}" ]] && grep -qE '^(<<<<<<<|=======|>>>>>>>)' "${f}"; then
        log "conflict markers still present in ${f}"
        continue
      fi
      git add -- "${f}"
    done

    if has_conflicts; then
      log "conflicts remain after Cursor round ${round}"
      ((round += 1))
      continue
    fi

    # Finish the merge if git still considers it in progress.
    if [[ -f .git/MERGE_HEAD ]]; then
      git -c core.editor=true commit --no-edit -m "chore(sync): merge upstream/${UPSTREAM_REF} into ${FEATURE_BRANCH}"
    fi
    return 0
  done
}

require_cmd git

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

if ! git remote get-url upstream >/dev/null 2>&1; then
  git remote add upstream "${UPSTREAM_URL}"
else
  git remote set-url upstream "${UPSTREAM_URL}"
fi

log "Fetching upstream/${UPSTREAM_REF} and origin/${FEATURE_BRANCH}"
git fetch --no-tags upstream "${UPSTREAM_REF}"
git fetch origin "${FEATURE_BRANCH}"

UPSTREAM_SHA="$(git rev-parse "upstream/${UPSTREAM_REF}")"
UPSTREAM_TAG="$(git describe --tags --abbrev=0 "upstream/${UPSTREAM_REF}" 2>/dev/null || true)"
SUGGESTED_TAG=""
if [[ -n "${UPSTREAM_TAG}" ]]; then
  SUGGESTED_TAG="${UPSTREAM_TAG}-public-api"
fi

log "upstream/${UPSTREAM_REF}=${UPSTREAM_SHA}"
log "suggested image tag=${SUGGESTED_TAG:-unknown}"

if git merge-base --is-ancestor "${UPSTREAM_SHA}" "origin/${FEATURE_BRANCH}"; then
  log "origin/${FEATURE_BRANCH} already contains upstream/${UPSTREAM_REF} — nothing to do"
  write_output "changed" "false"
  write_output "conflicted" "false"
  write_output "cursor_used" "false"
  write_output "upstream_sha" "${UPSTREAM_SHA}"
  write_output "suggested_tag" "${SUGGESTED_TAG}"
  write_output "sync_branch" "${SYNC_BRANCH}"
  exit 0
fi

log "Preparing ${SYNC_BRANCH} from origin/${FEATURE_BRANCH}"
git checkout -B "${SYNC_BRANCH}" "origin/${FEATURE_BRANCH}"

CURSOR_USED="false"
CONFLICTED="false"

set +e
git merge --no-ff --no-edit "upstream/${UPSTREAM_REF}"
MERGE_STATUS=$?
set -e

if [[ "${MERGE_STATUS}" -ne 0 ]]; then
  if ! has_conflicts; then
    die "merge failed without conflict markers (exit ${MERGE_STATUS})"
  fi
  CONFLICTED="true"
  log "Merge conflicts detected — invoking Cursor CLI"
  resolve_with_cursor
  CURSOR_USED="true"
fi

if has_conflicts || [[ -f .git/MERGE_HEAD ]]; then
  die "merge still incomplete after resolution attempt"
fi

log "Merge completed (conflicted=${CONFLICTED}, cursor_used=${CURSOR_USED})"

write_output "changed" "true"
write_output "conflicted" "${CONFLICTED}"
write_output "cursor_used" "${CURSOR_USED}"
write_output "upstream_sha" "${UPSTREAM_SHA}"
write_output "suggested_tag" "${SUGGESTED_TAG}"
write_output "sync_branch" "${SYNC_BRANCH}"

log "Merge ready on local branch ${SYNC_BRANCH} (publish is handled by the workflow)"
