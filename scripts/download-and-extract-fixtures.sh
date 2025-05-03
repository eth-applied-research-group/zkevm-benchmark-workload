#!/usr/bin/env bash
#
# download-and-extract-fixtures.sh
#
# Edit these two lines to change what you fetch and where it lands:
TAG="zkevm@v0.0.1"          # Release tag on github.com/ethereum/execution-spec-tests
DEST_DIR="./zkevm-fixtures"  # Folder where the tarball will be extracted
#
# ─────────────────────────────────────────────────────────────────────

set -euo pipefail

REPO="ethereum/execution-spec-tests"
ASSET_NAME="fixtures_zkevm.tar.gz"
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
