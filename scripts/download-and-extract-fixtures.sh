#!/usr/bin/env bash
#
# download-and-extract-fixtures.sh
#
# Downloads execution spec test fixtures for zkevm.
# By default, it fetches the latest release tag starting with 'zkevm@'.
# An optional argument can be provided to specify an exact tag.
#
# Usage:
#   ./scripts/download-and-extract-fixtures.sh [TAG]
#
# Example (latest):
#   ./scripts/download-and-extract-fixtures.sh
# Example (specific tag):
#   ./scripts/download-and-extract-fixtures.sh zkevm@v0.0.1
#

DEST_DIR="./zkevm-fixtures"  # Folder where the tarball will be extracted
#
# ─────────────────────────────────────────────────────────────────────

set -euo pipefail

REPO="ethereum/execution-spec-tests"
ASSET_NAME="fixtures_zkevm.tar.gz"

# Determine the tag to use
if [ -n "${1:-}" ]; then
  # Use the tag provided as the first argument
  TAG="$1"
  echo "ℹ️  Using specified tag: ${TAG}"
else
  # Find the latest tag with 'zkevm@' prefix
  echo "🔎  Finding the latest release tag with prefix 'zkevm@'..."
  LATEST_TAG=$( \
    curl -fsSL "https://api.github.com/repos/${REPO}/tags" | \
    jq -r '.[].name' | \
    grep '^zkevm@' | \
    sed 's/^zkevm@v//' | \
    sort -V | \
    tail -n 1 | \
    sed 's/^/zkevm@v/' \
  )
  if [[ -z "${LATEST_TAG}" ]]; then
    echo "❌  Could not find any release tags with prefix 'zkevm@' in ${REPO}" >&2
    exit 1
  fi
  TAG="${LATEST_TAG}"
  echo "ℹ️  Using latest found tag: ${TAG}"
fi

API_URL="https://api.github.com/repos/${REPO}/releases/tags/${TAG}"

echo "🔎  Getting release info for ${TAG} …"
DOWNLOAD_URL=$(
  curl -fsSL "${API_URL}" |
  jq -r ".assets[] | select(.name==\"${ASSET_NAME}\") | .browser_download_url"
)

if [[ -z "${DOWNLOAD_URL}" || "${DOWNLOAD_URL}" == "null" ]]; then
  echo "❌  Asset ${ASSET_NAME} not found in release ${TAG}" >&2
  exit 1
fi

echo "⬇️  Downloading ${ASSET_NAME} …"
curl -L -o "${ASSET_NAME}" "${DOWNLOAD_URL}"

echo "📂  Extracting to ${DEST_DIR}/"
mkdir -p "${DEST_DIR}"
tar -xzf "${ASSET_NAME}" -C "${DEST_DIR}"

echo "🗑️  Cleaning up ${ASSET_NAME}"
rm -f "${ASSET_NAME}"

echo "✅  Fixtures ready in ${DEST_DIR}"
