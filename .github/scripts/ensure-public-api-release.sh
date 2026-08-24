#!/usr/bin/env bash
# If upstream has a newer release than the fork's *-public-api tag, trigger Release Public API.
# Intended for the case where feature-public-api already contains upstream/main (nothing to merge)
# but upstream tagged a release after the last fork publish.
set -euo pipefail

UPSTREAM_REPO="${UPSTREAM_REPO:-dani-garcia/vaultwarden}"
FEATURE_BRANCH="${FEATURE_BRANCH:-feature-public-api}"
RELEASE_WORKFLOW="${RELEASE_WORKFLOW:-release-public-api.yml}"
DRY_RUN="${DRY_RUN:-false}"
REPO="${REPO:-${GITHUB_REPOSITORY:-}}"

log() { printf '==> %s\n' "$*"; }

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

command -v gh >/dev/null 2>&1 || { echo "ERROR: gh is required" >&2; exit 1; }
command -v git >/dev/null 2>&1 || { echo "ERROR: git is required" >&2; exit 1; }

UPSTREAM_VERSION="$(gh api "repos/${UPSTREAM_REPO}/releases/latest" --jq '.tag_name')"
UPSTREAM_VERSION="${UPSTREAM_VERSION#v}"
if [[ ! "${UPSTREAM_VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "ERROR: unexpected upstream version '${UPSTREAM_VERSION}'" >&2
  exit 1
fi

IMAGE_TAG="${UPSTREAM_VERSION}-public-api"
log "Latest upstream release: ${UPSTREAM_VERSION} (fork tag would be ${IMAGE_TAG})"

# Resolve the commit the upstream release tag points at (peel annotated tags).
OBJ_TYPE="$(gh api "repos/${UPSTREAM_REPO}/git/ref/tags/${UPSTREAM_VERSION}" --jq '.object.type')"
OBJ_SHA="$(gh api "repos/${UPSTREAM_REPO}/git/ref/tags/${UPSTREAM_VERSION}" --jq '.object.sha')"
if [[ "${OBJ_TYPE}" == "tag" ]]; then
  UPSTREAM_RELEASE_SHA="$(gh api "repos/${UPSTREAM_REPO}/git/tags/${OBJ_SHA}" --jq '.object.sha')"
else
  UPSTREAM_RELEASE_SHA="${OBJ_SHA}"
fi
log "Upstream release commit: ${UPSTREAM_RELEASE_SHA}"

git fetch origin "${FEATURE_BRANCH}"
# Ensure the release commit object exists locally (may already be present from sync).
if ! git cat-file -e "${UPSTREAM_RELEASE_SHA}^{commit}" 2>/dev/null; then
  git fetch "https://github.com/${UPSTREAM_REPO}.git" "${UPSTREAM_RELEASE_SHA}"
fi
if ! git merge-base --is-ancestor "${UPSTREAM_RELEASE_SHA}" "origin/${FEATURE_BRANCH}"; then
  log "origin/${FEATURE_BRANCH} does not contain upstream release commit yet — skip (sync first)"
  write_output "needed" "false"
  write_output "triggered" "false"
  write_output "image_tag" "${IMAGE_TAG}"
  write_output "upstream_version" "${UPSTREAM_VERSION}"
  write_output "reason" "feature-branch-missing-release-commit"
  exit 0
fi

if git ls-remote --exit-code --tags origin "refs/tags/${IMAGE_TAG}" >/dev/null 2>&1; then
  log "Tag ${IMAGE_TAG} already exists on origin — nothing to do"
  write_output "needed" "false"
  write_output "triggered" "false"
  write_output "image_tag" "${IMAGE_TAG}"
  write_output "upstream_version" "${UPSTREAM_VERSION}"
  write_output "reason" "tag-exists"
  exit 0
fi

log "Fork is missing ${IMAGE_TAG}"
write_output "needed" "true"
write_output "image_tag" "${IMAGE_TAG}"
write_output "upstream_version" "${UPSTREAM_VERSION}"
write_output "reason" "missing-tag"

if [[ "${DRY_RUN}" == "true" ]]; then
  log "Dry run — not triggering ${RELEASE_WORKFLOW}"
  write_output "triggered" "false"
  exit 0
fi

log "Triggering workflow '${RELEASE_WORKFLOW}' on ${FEATURE_BRANCH} (version=${UPSTREAM_VERSION})"
GH_ARGS=()
if [[ -n "${REPO}" ]]; then
  GH_ARGS+=(--repo "${REPO}")
fi
gh workflow run "${RELEASE_WORKFLOW}" \
  "${GH_ARGS[@]}" \
  --ref "${FEATURE_BRANCH}" \
  -f "version=${UPSTREAM_VERSION}" \
  -f "dry_run=false"

write_output "triggered" "true"
log "Release workflow dispatched"
