# Installation Guide

Complete installation instructions for all platforms and methods.

## VS Code Marketplace (Recommended)

The easiest way to install Pointbreak:

1. Open VS Code (or Cursor, Windsurf, VS Codium)
2. Go to Extensions (Cmd+Shift+X / Ctrl+Shift+X)
3. Search for "Pointbreak"
4. Publisher should be: `pointbreak`
5. Click "Install"

**Direct link:** [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=pointbreak.pointbreak)

## Manual Installation

### Step 1: Download VSIX

Download the platform-specific VSIX from [GitHub Releases](https://github.com/withpointbreak/pointbreak/releases):

| Platform | File Pattern | Architecture |
|----------|--------------|--------------|
| macOS (Apple Silicon) | `pointbreak-darwin-arm64-*.vsix` | ARM64 (M1/M2/M3) |
| macOS (Intel) | `pointbreak-darwin-x64-*.vsix` | x64 |
| Linux x64 | `pointbreak-linux-x64-*.vsix` | x64 |
| Linux ARM64 | `pointbreak-linux-arm64-*.vsix` | ARM64 |
| Windows x64 | `pointbreak-win32-x64-*.vsix` | x64 |
| Windows ARM64 | `pointbreak-win32-arm64-*.vsix` | ARM64 |

### Step 2: Install VSIX

**Via Command Line:**
```bash
code --install-extension path/to/pointbreak-*.vsix
```

**Via VS Code UI:**
1. Open VS Code
2. Go to Extensions (Cmd+Shift+X / Ctrl+Shift+X)
3. Click the "..." menu (top right)
4. Select "Install from VSIX..."
5. Choose the downloaded file

## Verifying Installation

After installation:

1. Open the Output panel: **View → Output**
2. Select **"Pointbreak MCP Server"** from the dropdown
3. You should see: `Pointbreak MCP server started`

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

- ✅ CodeLLDB (Rust, C, C++)
- ✅ debugpy (Python)
- ✅ Node Debug (JavaScript, TypeScript)
- ✅ vscode-js-debug (JavaScript, TypeScript, Chrome)
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
