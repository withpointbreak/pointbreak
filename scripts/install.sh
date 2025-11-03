#!/bin/sh
# Pointbreak Installation Script
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.sh | sh
#
# Options:
#   --version=VERSION   Install specific version (e.g., --version=v0.2.0)
#   --prefix=PATH       Install to custom directory (default: ~/.local/bin)
#   --no-verify         Skip checksum verification (not recommended)

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
REPO="withpointbreak/pointbreak"
INSTALL_DIR="${HOME}/.local/bin"
VERSION="latest"
VERIFY_CHECKSUM=true

# Parse command line arguments
for arg in "$@"; do
    case $arg in
        --version=*)
            VERSION="${arg#*=}"
            shift
            ;;
        --prefix=*)
            INSTALL_DIR="${arg#*=}"
            shift
            ;;
        --no-verify)
            VERIFY_CHECKSUM=false
            shift
            ;;
        *)
            echo "${RED}Unknown option: $arg${NC}"
            echo "Usage: install.sh [--version=VERSION] [--prefix=PATH] [--no-verify]"
            exit 1
            ;;
    esac
done

echo "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo "${BLUE}  Pointbreak Installer${NC}"
echo "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# Detect platform
detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Darwin)
            OS_NAME="darwin"
            ;;
        Linux)
            # Detect if Alpine (musl) or regular Linux (glibc)
            if [ -f /etc/os-release ]; then
                . /etc/os-release
                if [ "$ID" = "alpine" ]; then
                    OS_NAME="alpine"
                else
                    OS_NAME="linux"
                fi
            elif ldd --version 2>&1 | grep -q musl; then
                OS_NAME="alpine"
            else
                OS_NAME="linux"
            fi
            ;;
        *)
            echo "${RED}Error: Unsupported operating system: $OS${NC}"
            echo "This script only supports macOS and Linux."
            echo "For Windows, use: irm https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.ps1 | iex"
            exit 1
            ;;
    esac

    case "$ARCH" in
        x86_64|amd64)
            ARCH_NAME="x64"
            ;;
        aarch64|arm64)
            ARCH_NAME="arm64"
            ;;
        *)
            echo "${RED}Error: Unsupported architecture: $ARCH${NC}"
            echo "Supported architectures: x86_64, aarch64"
            exit 1
            ;;
    esac

    PLATFORM="${OS_NAME}-${ARCH_NAME}"
    echo "${GREEN}✓${NC} Detected platform: ${BLUE}$PLATFORM${NC}"
}

# Get download URL
get_download_url() {
    if [ "$VERSION" = "latest" ]; then
        echo "  Fetching latest release..."
        API_URL="https://api.github.com/repos/${REPO}/releases/latest"
    else
        echo "  Using version: $VERSION"
        API_URL="https://api.github.com/repos/${REPO}/releases/tags/${VERSION}"
    fi

    # Fetch release info
    if command -v curl >/dev/null 2>&1; then
        RELEASE_JSON=$(curl -fsSL "$API_URL")
    elif command -v wget >/dev/null 2>&1; then
        RELEASE_JSON=$(wget -qO- "$API_URL")
    else
        echo "${RED}Error: Neither curl nor wget found. Please install one of them.${NC}"
        exit 1
    fi

    # Extract version
    RELEASE_TAG=$(echo "$RELEASE_JSON" | grep -o '"tag_name": *"[^"]*"' | head -1 | sed 's/.*: *"\(.*\)".*/\1/')

    if [ -z "$RELEASE_TAG" ]; then
        echo "${RED}Error: Could not fetch release information${NC}"
        if [ "$VERSION" != "latest" ]; then
            echo "Version $VERSION not found. Check: https://github.com/${REPO}/releases"
        fi
        exit 1
    fi

    echo "${GREEN}✓${NC} Found version: ${BLUE}$RELEASE_TAG${NC}"

    # Construct download URLs
    BINARY_NAME="pointbreak-${PLATFORM}"
    BINARY_URL="https://github.com/${REPO}/releases/download/${RELEASE_TAG}/${BINARY_NAME}"
    CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${RELEASE_TAG}/checksums.txt"
}

# Download file
download_file() {
    URL=$1
    OUTPUT=$2

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$OUTPUT" "$URL"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$OUTPUT" "$URL"
    fi
}

# Download and verify binary
download_binary() {
    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT

    echo ""
    echo "Downloading binary..."

    BINARY_PATH="${TMP_DIR}/${BINARY_NAME}"
    download_file "$BINARY_URL" "$BINARY_PATH"

    if [ ! -f "$BINARY_PATH" ]; then
        echo "${RED}Error: Download failed${NC}"
        exit 1
    fi

    echo "${GREEN}✓${NC} Downloaded: $(du -h "$BINARY_PATH" | cut -f1)"

    # Verify checksum
    if [ "$VERIFY_CHECKSUM" = true ]; then
        echo ""
        echo "Verifying checksum..."

        CHECKSUMS_PATH="${TMP_DIR}/checksums.txt"
        download_file "$CHECKSUMS_URL" "$CHECKSUMS_PATH"

        if [ ! -f "$CHECKSUMS_PATH" ]; then
            echo "${YELLOW}Warning: Could not download checksums file${NC}"
            echo "Skipping checksum verification"
        else
            # Extract checksum for our binary
            EXPECTED_CHECKSUM=$(grep "$BINARY_NAME" "$CHECKSUMS_PATH" | awk '{print $1}')

            if [ -z "$EXPECTED_CHECKSUM" ]; then
                echo "${YELLOW}Warning: Checksum not found in checksums.txt${NC}"
                echo "Skipping checksum verification"
            else
                # Calculate actual checksum
                if command -v sha256sum >/dev/null 2>&1; then
                    ACTUAL_CHECKSUM=$(sha256sum "$BINARY_PATH" | awk '{print $1}')
                elif command -v shasum >/dev/null 2>&1; then
                    ACTUAL_CHECKSUM=$(shasum -a 256 "$BINARY_PATH" | awk '{print $1}')
                else
                    echo "${YELLOW}Warning: sha256sum/shasum not found${NC}"
                    echo "Skipping checksum verification"
                    ACTUAL_CHECKSUM=""
                fi

                if [ -n "$ACTUAL_CHECKSUM" ]; then
                    if [ "$ACTUAL_CHECKSUM" = "$EXPECTED_CHECKSUM" ]; then
                        echo "${GREEN}✓${NC} Checksum verified"
                    else
                        echo "${RED}Error: Checksum mismatch!${NC}"
                        echo "Expected: $EXPECTED_CHECKSUM"
                        echo "Got:      $ACTUAL_CHECKSUM"
                        exit 1
                    fi
                fi
            fi
        fi
    fi

    # Install binary
    echo ""
    echo "Installing to: ${BLUE}$INSTALL_DIR${NC}"

    mkdir -p "$INSTALL_DIR"

    TARGET_PATH="${INSTALL_DIR}/pointbreak"
    cp "$BINARY_PATH" "$TARGET_PATH"
    chmod +x "$TARGET_PATH"

    echo "${GREEN}✓${NC} Installed successfully"
}

# Check if directory is in PATH
check_path() {
    echo ""

    case ":$PATH:" in
        *":$INSTALL_DIR:"*)
            echo "${GREEN}✓${NC} Install directory is in PATH"
            ;;
        *)
            echo "${YELLOW}⚠${NC}  Install directory is not in PATH"
            echo ""
            echo "To add it to your PATH, add this line to your shell config:"
            echo ""

            if [ -n "$ZSH_VERSION" ] || [ -f "$HOME/.zshrc" ]; then
                echo "  ${BLUE}echo 'export PATH=\"\$PATH:$INSTALL_DIR\"' >> ~/.zshrc${NC}"
                echo "  ${BLUE}source ~/.zshrc${NC}"
            elif [ -n "$BASH_VERSION" ] || [ -f "$HOME/.bashrc" ]; then
                echo "  ${BLUE}echo 'export PATH=\"\$PATH:$INSTALL_DIR\"' >> ~/.bashrc${NC}"
                echo "  ${BLUE}source ~/.bashrc${NC}"
            else
                echo "  ${BLUE}export PATH=\"\$PATH:$INSTALL_DIR\"${NC}"
            fi
            ;;
    esac
}

# Print next steps
print_next_steps() {
    echo ""
    echo "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo "${GREEN}  Installation Complete!${NC}"
    echo "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    echo "Verify installation:"
    echo "  ${BLUE}pointbreak --version${NC}"
    echo ""
    echo "Next steps:"
    echo "  1. Install the VS Code extension (if not already installed)"
    echo "  2. Configure your AI assistant to use Pointbreak MCP server"
    echo "  3. See setup guides: ${BLUE}https://github.com/${REPO}/tree/main/docs${NC}"
    echo ""
}

# Main installation flow
main() {
    detect_platform
    get_download_url
    download_binary
    check_path
    print_next_steps
}

main
