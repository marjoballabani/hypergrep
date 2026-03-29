#!/bin/bash
# Install hypergrep from source
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/marjoballabani/hypergrep/main/install.sh | bash
#
# Or manually:
#   git clone https://github.com/mballabani/hypergrep.git
#   cd hypergrep
#   ./install.sh

set -e

BOLD="\033[1m"
GREEN="\033[32m"
RED="\033[31m"
RESET="\033[0m"

info() { echo -e "${GREEN}${BOLD}$1${RESET}"; }
error() { echo -e "${RED}${BOLD}Error: $1${RESET}" >&2; exit 1; }

# Check Rust toolchain
if ! command -v cargo &> /dev/null; then
    error "Rust toolchain not found. Install it: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi

RUST_VERSION=$(rustc --version | awk '{print $2}')
info "Found Rust $RUST_VERSION"

# Build
info "Building hypergrep (release mode)..."
cargo build --release

# Install to cargo bin directory
CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
info "Installing to $CARGO_BIN..."

cp target/release/hypergrep "$CARGO_BIN/hypergrep"
cp target/release/hypergrep-daemon "$CARGO_BIN/hypergrep-daemon"

chmod +x "$CARGO_BIN/hypergrep"
chmod +x "$CARGO_BIN/hypergrep-daemon"

# Verify
if command -v hypergrep &> /dev/null; then
    VERSION=$(hypergrep --version)
    info "Installed: $VERSION"
    echo ""
    echo "Usage:"
    echo "  hypergrep \"pattern\" src/              # text search"
    echo "  hypergrep -s \"pattern\" src/            # structural search"
    echo "  hypergrep --layer 1 --json \"fn\" src/   # semantic + JSON"
    echo "  hypergrep --impact \"symbol\" src/       # impact analysis"
    echo "  hypergrep --exists \"redis\" src/        # existence check"
    echo "  hypergrep --model \"\" src/              # codebase overview"
    echo "  hypergrep --help                       # full help"
else
    echo ""
    echo "Binaries installed to $CARGO_BIN"
    echo "Make sure $CARGO_BIN is in your PATH:"
    echo "  export PATH=\"$CARGO_BIN:\$PATH\""
fi
