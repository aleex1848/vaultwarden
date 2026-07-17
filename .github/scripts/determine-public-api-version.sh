#!/usr/bin/env bash
# Resolve Vaultwarden upstream version and the public-api image/git tag.
set -euo pipefail

UPSTREAM_REPO="${UPSTREAM_REPO:-dani-garcia/vaultwarden}"
VERSION_OVERRIDE="${VERSION_OVERRIDE:-}"

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
  printf '%s=%s\n' "${key}" "${value}"
}

if [[ -n "${VERSION_OVERRIDE}" ]]; then
  UPSTREAM_VERSION="${VERSION_OVERRIDE#v}"
  UPSTREAM_VERSION="${UPSTREAM_VERSION%-public-api}"
  log "Using version override: ${UPSTREAM_VERSION}"
else
  require_gh() { command -v gh >/dev/null 2>&1 || { echo "gh is required" >&2; exit 1; }; }
  require_gh
  UPSTREAM_VERSION="$(gh api "repos/${UPSTREAM_REPO}/releases/latest" --jq '.tag_name')"
  UPSTREAM_VERSION="${UPSTREAM_VERSION#v}"
  log "Latest upstream release: ${UPSTREAM_VERSION}"
fi

if [[ ! "${UPSTREAM_VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "ERROR: unexpected upstream version '${UPSTREAM_VERSION}'" >&2
  exit 1
fi

IMAGE_TAG="${UPSTREAM_VERSION}-public-api"

write_output "upstream_version" "${UPSTREAM_VERSION}"
write_output "image_tag" "${IMAGE_TAG}"
write_output "floating_tag" "public-api"
