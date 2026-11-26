#!/bin/bash
# Local machine script to pull releases from mbpro and manage gold link
# Usage: ./manage-releases.sh [pull|gold|list|use <name>]

set -euo pipefail

MBPRO_HOST="${MBPRO_HOST:-mbpro.local}"
MBPRO_USER="${MBPRO_USER:-$(whoami)}"
RELEASES_DIR="${HOME}/.prime-agent/releases"
GOLD_LINK="${HOME}/.local/bin/prime-agent"

mkdir -p "${RELEASES_DIR}"
mkdir -p "$(dirname "${GOLD_LINK}")"

CMD="${1:-list}"

case "${CMD}" in
    pull)
        echo "Pulling releases from ${MBPRO_USER}@${MBPRO_HOST}..."
        rsync -avz --progress "${MBPRO_USER}@${MBPRO_HOST}:.prime-agent/releases/" "${RELEASES_DIR}/"
        ;;
    gold)
        # Show current gold
        if [ -L "${GOLD_LINK}" ]; then
            CURRENT=$(readlink -f "${GOLD_LINK}")
            echo "Current gold: $(basename "${CURRENT}")"
        else
            echo "No gold link set"
        fi
        # List available
        echo ""
        echo "Available releases:"
        ls -lt "${RELEASES_DIR}"/prime-agent-* 2>/dev/null | head -10 | nl
        ;;
    list)
        ls -lt "${RELEASES_DIR}"/prime-agent-* 2>/dev/null | head -20
        ;;
    use)
        NAME="${2:-}"
        if [ -z "${NAME}" ]; then
            echo "Usage: $0 use <release-name>"
            exit 1
        fi
        TARGET="${RELEASES_DIR}/${NAME}"
        if [ ! -f "${TARGET}" ]; then
            echo "Release not found: ${TARGET}"
            exit 1
        fi
        ln -sf "${TARGET}" "${GOLD_LINK}"
        echo "Gold link updated: ${GOLD_LINK} -> ${NAME}"
        "${GOLD_LINK}" --version
        ;;
    latest)
        LATEST=$(ls -t "${RELEASES_DIR}"/prime-agent-* 2>/dev/null | head -1)
        if [ -z "${LATEST}" ]; then
            echo "No releases found"
            exit 1
        fi
        ln -sf "${LATEST}" "${GOLD_LINK}"
        echo "Gold link updated to latest: $(basename "${LATEST}")"
        "${GOLD_LINK}" --version
        ;;
    *)
        echo "Usage: $0 [pull|gold|list|use <name>|latest]"
        exit 1
        ;;
esac
