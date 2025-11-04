# Installation Guide

Complete installation instructions for all platforms and methods.

**📌 Important:** Your installation depends on which AI assistant you're using!

- **GitHub Copilot or Cursor** → Install extension only (stop after [Step 1](#step-1-install-vs-code-extension))
- **Claude Code, Codex, Windsurf, or other external AI assistants** → Install extension ([Step 1](#step-1-install-vs-code-extension)) + CLI ([Step 2](#step-2-install-cli-external-ai-assistants-only))

**Don't skip the CLI installation if you're using an external AI assistant!**

---

## Step 1: Install VS Code Extension

### VS Code Marketplace (Recommended)

The easiest way to install Pointbreak:

1. Open VS Code (or Cursor, Windsurf, VS Codium)
2. Go to Extensions (Cmd+Shift+X / Ctrl+Shift+X)
3. Search for "Pointbreak"
4. Publisher should be: `pointbreak`
5. Click "Install"

**Direct link:** [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=pointbreak.pointbreak)

### Manual Installation (Alternative)

**Step 1a: Download VSIX**

Download the platform-specific VSIX from [GitHub Releases](https://github.com/withpointbreak/pointbreak/releases):

| Platform              | File Pattern                     | Architecture     |
| --------------------- | -------------------------------- | ---------------- |
| macOS (Apple Silicon) | `pointbreak-darwin-arm64-*.vsix` | ARM64 (M1/M2/M3) |
| macOS (Intel)         | `pointbreak-darwin-x64-*.vsix`   | x64              |
| Linux x64             | `pointbreak-linux-x64-*.vsix`    | x64              |
| Linux ARM64           | `pointbreak-linux-arm64-*.vsix`  | ARM64            |
| Windows x64           | `pointbreak-win32-x64-*.vsix`    | x64              |
| Windows ARM64         | `pointbreak-win32-arm64-*.vsix`  | ARM64            |

**Step 1b: Install VSIX**

**Via Command Line:**

Visual Studio Code, Cursor, Windsurf (or compatible editors) must be installed and in your PATH.

```bash
code --install-extension path/to/pointbreak-*.vsix
```

```bash
code-insiders --install-extension path/to/pointbreak-*.vsix
```

```bash
cursor --install-extension path/to/pointbreak-*.vsix
```

```bash
surf --install-extension path/to/pointbreak-*.vsix
```

**Via VS Code UI:**

1. Open VS Code
2. Go to Extensions (Cmd+Shift+X / Ctrl+Shift+X)
3. Click the "..." menu (top right)
4. Select "Install from VSIX..."
5. Choose the downloaded file

## Step 2: Install CLI (External AI Assistants Only)

**Note:** Skip this step if you're using GitHub Copilot or Cursor - the extension is all you need!

If you're using **Claude Code, Codex, Windsurf**, or another external AI assistant, you need to install the Pointbreak CLI in addition to the VS Code extension.

### Install Script (Recommended)

**macOS / Linux:**
```bash
curl -fsSL https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.sh | sh
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.ps1 | iex
```

### What It Does

The install script:
1. Detects your platform and architecture
2. Downloads the appropriate binary from GitHub Releases
3. Verifies the binary with SHA256 checksums
4. Installs to user directory (no sudo required)
5. Checks PATH configuration

### Installation Paths

After installation, the binary is located at:

**macOS / Linux:**
```
~/.local/bin/pointbreak
```

**Windows:**
```
%LOCALAPPDATA%\Pointbreak\bin\pointbreak.exe
```

### Verify CLI Installation

```bash
# Check binary is installed
pointbreak --version
# Should output: pointbreak X.X.X

# macOS / Linux - Find binary location
which pointbreak
# Should show: /Users/username/.local/bin/pointbreak

# Windows - Find binary location
where.exe pointbreak
# Should show: C:\Users\username\AppData\Local\Pointbreak\bin\pointbreak.exe
```

### PATH Configuration

The install script checks if the binary location is in your PATH. If not, you'll see instructions to add it.

**macOS / Linux** - Add to `~/.bashrc`, `~/.zshrc`, or `~/.profile`:
```bash
export PATH="$HOME/.local/bin:$PATH"
```

**Windows** - The install script offers to add to PATH automatically.

### Next Steps After CLI Installation

1. Configure your AI assistant's MCP settings - See [MCP Configuration Reference](mcp-configuration.md)
2. Specifically for Claude Code - See [AI Assistants Guide](ai-assistants.md#claude-code)

## Verifying Installation

After installation:

1. Open the Output panel: **View → Output**
2. Select **"Pointbreak"** from the dropdown
3. You should see: `Pointbreak debug bridge started successfully`

If you see errors, continue to the Troubleshooting section below.

## Updating

### Marketplace Version

Updates happen automatically if you have auto-update enabled:

1. Go to VS Code Settings
2. Search for "auto update"
3. Ensure "Extensions: Auto Update" is enabled

### Manual VSIX Version

1. Download the latest VSIX
2. Install it over the old version (same command as installation)
3. Restart VS Code

## Uninstalling

**Via VS Code UI:**

1. Go to Extensions
2. Find Pointbreak
3. Click the gear icon
4. Select "Uninstall"
5. Restart VS Code

**Via Command Line:**

```bash
code --uninstall-extension pointbreak.pointbreak
```

## Troubleshooting Installation

### Extension Won't Install

**Problem:** "Extension is not compatible with this version of VS Code"

**Solution:** Update VS Code to version 1.74.0 or later.

### Extension Not Appearing

**Problem:** Can't find Pointbreak in marketplace

**Solution:**

1. Check you're searching for exactly "Pointbreak"
2. Look for publisher: `pointbreak`
3. Try the direct marketplace link

### Installation Succeeds But Extension Doesn't Work

**Problem:** Extension installed but Output panel shows errors

**Solution:** See [Troubleshooting Guide](troubleshooting.md) for detailed diagnostics.

### Wrong Architecture Downloaded

**Problem:** Downloaded x64 but need ARM64 (or vice versa)

**Solution:**

Check your architecture:

**macOS:**

```bash
uname -m
# arm64 = Apple Silicon
# x86_64 = Intel
```

**Linux:**

```bash
uname -m
# x86_64 = x64
# aarch64 = ARM64
```

**Windows:**

```powershell
echo $env:PROCESSOR_ARCHITECTURE
# AMD64 = x64
# ARM64 = ARM64
```

Download the matching VSIX.

## Compatibility

### Supported Editors

- ✅ VS Code 1.74.0+
- ✅ Cursor (latest)
- ✅ Windsurf
- ✅ VS Codium
- ✅ Other VS Code compatible editors with extension support

### Supported Platforms

- ✅ macOS 10.15+ (Catalina or later)
- ✅ Windows 10+ (x64 and ARM64)
- ✅ Linux (recent distributions with glibc 2.27+)

### Supported Debug Adapters

Pointbreak works with any Debug Adapter Protocol (DAP) compliant debugger:

- ✅ debugpy (Python)
- ✅ vscode-js-debug (JavaScript, TypeScript, Chrome)
- ✅ CodeLLDB (Rust, C, C++)
- ✅ Delve (Go)
- ✅ And many more...

## Next Steps

After installation:

1. **Install a debug adapter** for your language (if not already installed)
2. **Set up your AI assistant** - See [AI Assistants Guide](ai-assistants.md)
3. **Try your first debugging session** - See [Getting Started](getting-started.md)

## Questions?

- **Installation issues:** [GitHub Issues](https://github.com/withpointbreak/pointbreak/issues)
- **General questions:** [GitHub Discussions](https://github.com/withpointbreak/pointbreak/discussions)
- **Website:** [withpointbreak.com](https://withpointbreak.com)
