#!/bin/bash
# Hypergrep installer - downloads the right binary for your platform
#
# Usage:
#   curl -sSfL https://github.com/marjoballabani/hypergrep/releases/latest/download/hypergrep-installer.sh | sh

set -e

REPO="marjoballabani/hypergrep"
INSTALL_DIR="${HYPERGREP_INSTALL_DIR:-/usr/local/bin}"

BOLD="\033[1m"
GREEN="\033[32m"
RED="\033[31m"
YELLOW="\033[33m"
RESET="\033[0m"

info() { echo -e "${GREEN}${BOLD}$1${RESET}"; }
warn() { echo -e "${YELLOW}$1${RESET}"; }
error() { echo -e "${RED}${BOLD}Error: $1${RESET}" >&2; exit 1; }

# Detect platform
OS=$(uname -s)
ARCH=$(uname -m)

case "${OS}" in
    Darwin)
        case "${ARCH}" in
            x86_64)  TARGET="x86_64-apple-darwin" ;;
            arm64)   TARGET="aarch64-apple-darwin" ;;
            aarch64) TARGET="aarch64-apple-darwin" ;;
            *)       error "Unsupported macOS architecture: ${ARCH}" ;;
        esac
        ;;
    Linux)
        case "${ARCH}" in
            x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
            aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
            arm64)   TARGET="aarch64-unknown-linux-gnu" ;;
            *)       error "Unsupported Linux architecture: ${ARCH}" ;;
        esac
        ;;
    *)
        error "Unsupported OS: ${OS}. Use 'cargo install' instead."
        ;;
esac

info "Detected platform: ${OS} ${ARCH} (${TARGET})"

# Get latest version
if command -v curl &> /dev/null; then
    FETCH="curl -sSfL"
elif command -v wget &> /dev/null; then
    FETCH="wget -qO-"
else
    error "Neither curl nor wget found. Install one and retry."
fi

info "Fetching latest release..."
LATEST_URL="https://api.github.com/repos/${REPO}/releases/latest"
VERSION=$(${FETCH} "${LATEST_URL}" 2>/dev/null | grep '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')

if [ -z "${VERSION}" ]; then
    error "Could not determine latest version. Check https://github.com/${REPO}/releases"
fi

info "Latest version: ${VERSION}"

# Download
ARCHIVE="hypergrep-${TARGET}.tar.gz"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"

TMPDIR=$(mktemp -d)
trap "rm -rf ${TMPDIR}" EXIT

info "Downloading ${ARCHIVE}..."
if command -v curl &> /dev/null; then
    curl -sSfL "${DOWNLOAD_URL}" -o "${TMPDIR}/${ARCHIVE}"
else
    wget -q "${DOWNLOAD_URL}" -O "${TMPDIR}/${ARCHIVE}"
fi

# Extract
info "Extracting..."
tar xzf "${TMPDIR}/${ARCHIVE}" -C "${TMPDIR}"

# Install
if [ -w "${INSTALL_DIR}" ]; then
    cp "${TMPDIR}/hypergrep" "${INSTALL_DIR}/hypergrep"
    cp "${TMPDIR}/hypergrep-daemon" "${INSTALL_DIR}/hypergrep-daemon" 2>/dev/null || true
    chmod +x "${INSTALL_DIR}/hypergrep"
    chmod +x "${INSTALL_DIR}/hypergrep-daemon" 2>/dev/null || true
else
    warn "Need sudo to install to ${INSTALL_DIR}"
    sudo cp "${TMPDIR}/hypergrep" "${INSTALL_DIR}/hypergrep"
    sudo cp "${TMPDIR}/hypergrep-daemon" "${INSTALL_DIR}/hypergrep-daemon" 2>/dev/null || true
    sudo chmod +x "${INSTALL_DIR}/hypergrep"
    sudo chmod +x "${INSTALL_DIR}/hypergrep-daemon" 2>/dev/null || true
fi

# Verify
if command -v hypergrep &> /dev/null; then
    INSTALLED_VERSION=$(hypergrep --version)
    info "Installed: ${INSTALLED_VERSION}"
else
    info "Installed to ${INSTALL_DIR}/hypergrep"
    warn "Make sure ${INSTALL_DIR} is in your PATH"
fi

echo ""
echo "Quick start:"
echo "  hypergrep \"pattern\" src/              # text search"
echo "  hypergrep -s \"pattern\" src/            # structural search"
echo "  hypergrep --layer 1 --json \"fn\" src/   # semantic + JSON"
echo "  hypergrep --impact \"symbol\" src/       # impact analysis"
echo "  hypergrep --help                       # full help"
