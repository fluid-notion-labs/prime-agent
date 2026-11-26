#!/bin/bash
# mbpro.local build script for prime-agent feature branches
# Usage: ./build-feature.sh <branch-name> <keywords>

set -euo pipefail

REPO_DIR="${HOME}/Projects/nursery/prime-agent"  # Adjust to your mbpro clone location
RELEASES_DIR="${HOME}/.prime-agent/releases"
GOLD_LINK="${HOME}/.local/bin/prime-agent"

mkdir -p "${RELEASES_DIR}"

BRANCH="${1:-}"
KEYWORDS="${2:-}"

if [ -z "${BRANCH}" ] || [ -z "${KEYWORDS}" ]; then
    echo "Usage: $0 <branch-name> <keywords>"
    echo "Example: $0 feat/pa-core-429-retry '429-retry provider-failure'"
    exit 1
fi

cd "${REPO_DIR}"

# Fetch and checkout
git fetch origin "${BRANCH}"
git checkout "${BRANCH}"
git pull origin "${BRANCH}"

# Build release
echo "Building release for ${BRANCH}..."
cargo build --release --bin prime-agent

# Copy to releases with keywords in name
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
KEYWORDS_SAFE=$(echo "${KEYWORDS}" | tr ' ' '-' | tr '[:upper:]' '[:lower:]')
RELEASE_NAME="prime-agent-${TIMESTAMP}-${KEYWORDS_SAFE}"
cp target/release/prime-agent "${RELEASES_DIR}/${RELEASE_NAME}"

echo "Built: ${RELEASES_DIR}/${RELEASE_NAME}"

# Create/update gold symlink if this is marked as gold
if [ "${3:-}" = "gold" ]; then
    ln -sf "${RELEASES_DIR}/${RELEASE_NAME}" "${GOLD_LINK}"
    echo "Updated gold link: ${GOLD_LINK} -> ${RELEASE_NAME}"
fi

# List recent releases
echo ""
echo "Recent releases:"
ls -lt "${RELEASES_DIR}"/prime-agent-* | head -10
