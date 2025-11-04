# MCP Configuration Reference

Complete reference for configuring Model Context Protocol (MCP) servers with different AI assistants.

## Overview

Pointbreak uses the Model Context Protocol (MCP) to communicate with AI assistants. Some AI assistants (GitHub Copilot, Cursor) auto-register MCP servers, while others require manual configuration.

### Auto-Registration (No Configuration Needed)

These AI assistants automatically discover Pointbreak when you install the VS Code extension:

- **GitHub Copilot** - Built into VS Code
- **Cursor** - Built-in AI agent

### Manual Configuration Required

These AI assistants need explicit MCP configuration:

- **Claude Code** - Use CLI command `claude mcp add`
- **Codex** - Requires MCP settings
- **Windsurf** - Requires MCP settings
- **Other MCP clients** - Varies by client

---

## Claude Code

Claude Code uses a CLI-based approach for configuring MCP servers, not configuration files.

### Add Pointbreak MCP Server

Use the `claude mcp add` command to register Pointbreak:

```bash
claude mcp add --transport stdio pointbreak -- pointbreak
```

**What this does:**
- Registers Pointbreak as an MCP server named "pointbreak"
- Uses stdio transport for communication
- Runs the `pointbreak` binary from your PATH

### Using a Custom Binary Path

If Pointbreak is not in your PATH, specify the full path:

```bash
# macOS / Linux
claude mcp add --transport stdio pointbreak -- /Users/username/.local/bin/pointbreak

# Windows
claude mcp add --transport stdio pointbreak -- C:\Users\username\AppData\Local\Pointbreak\bin\pointbreak.exe
```

### Verification

List configured MCP servers:

```bash
claude mcp list
```

You should see `pointbreak` in the output.

Or start Claude Code and use the `/mcp` command to view configured servers.

### Troubleshooting

**Claude can't find Pointbreak MCP server**:
1. Verify the CLI is installed: `pointbreak --version`
2. Check PATH includes binary location: `which pointbreak` (macOS/Linux) or `where.exe pointbreak` (Windows)
3. List configured servers: `claude mcp list`
4. Try removing and re-adding: `claude mcp remove pointbreak` then re-add

**Binary not found**:
1. Run install script: `curl -fsSL https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.sh | sh`
2. Verify installation: `pointbreak --version`
3. Check PATH includes `~/.local/bin` (macOS/Linux) or `%LOCALAPPDATA%\Pointbreak\bin` (Windows)

**Managing MCP servers**:
- List all: `claude mcp list`
- Get details: `claude mcp get pointbreak`
- Remove: `claude mcp remove pointbreak`

---

## Codex

Codex also uses a CLI-based approach for configuring MCP servers.

### Add Pointbreak MCP Server

Use the `codex mcp add` command to register Pointbreak:

```bash
codex mcp add pointbreak -- pointbreak
```

**What this does:**
- Registers Pointbreak as an MCP server named "pointbreak"
- Uses stdio transport for communication
- Runs the `pointbreak` binary from your PATH

### Using a Custom Binary Path

If Pointbreak is not in your PATH, specify the full path:

```bash
# macOS / Linux
codex mcp add pointbreak -- /Users/username/.local/bin/pointbreak

# Windows
codex mcp add pointbreak -- C:\Users\username\AppData\Local\Pointbreak\bin\pointbreak.exe
```

### Verification

Start Codex and use the `/mcp` command to view configured servers. You should see Pointbreak listed.

Configuration is shared between the Codex CLI and IDE extension.

---

## Windsurf

### Configuration File Location

Windsurf stores MCP configuration in its settings. The exact location depends on your platform:

**macOS**:
```
~/Library/Application Support/Windsurf/User/settings.json
```

**Linux**:
```
~/.config/Windsurf/User/settings.json
```

**Windows**:
```
%APPDATA%\Windsurf\User\settings.json
```

### Configuration Format

Add to your Windsurf settings:

```json
{
  "mcp.servers": {
    "pointbreak": {
      "command": "/Users/username/.local/bin/pointbreak"
    }
  }
}
```

Use the same binary paths as shown in Claude Code section.

---

## Generic MCP Clients

### Standard MCP Configuration Format

Most MCP-compatible clients follow this format:

```json
{
  "mcpServers": {
    "pointbreak": {
      "command": "/path/to/pointbreak-binary",
      "args": [],
      "env": {}
    }
  }
}
```

**Parameters**:
- `command` (required): Absolute path to Pointbreak binary
- `args` (optional): Command-line arguments array
- `env` (optional): Environment variables object

### Finding Your Binary

After running the install script, the binary is located at:

**macOS / Linux**:
- Default: `~/.local/bin/pointbreak`
- Find it: `which pointbreak`

**Windows**:
- Default: `%LOCALAPPDATA%\Pointbreak\bin\pointbreak.exe`
- Find it: `where.exe pointbreak`

### Verify Installation

```bash
# Check binary exists
pointbreak --version

# macOS / Linux - check PATH
echo $PATH | grep -q ".local/bin" && echo "✓ Path OK" || echo "✗ Add ~/.local/bin to PATH"

# Windows - check PATH
$env:PATH -split ';' | Select-String "Pointbreak"
```

---

## Installation Paths Reference

### Binary Locations

**After running install script:**

| Platform | Binary Path |
|----------|-------------|
| macOS (Intel/ARM) | `~/.local/bin/pointbreak` |
| Linux (x64/ARM64) | `~/.local/bin/pointbreak` |
| Linux Alpine | `~/.local/bin/pointbreak` |
| Windows (x64/ARM64) | `%LOCALAPPDATA%\Pointbreak\bin\pointbreak.exe` |

**Expand to absolute paths:**

```bash
# macOS / Linux
echo ~/.local/bin/pointbreak
# Example: /Users/kevin/.local/bin/pointbreak

# Windows (PowerShell)
[System.Environment]::ExpandEnvironmentVariables("%LOCALAPPDATA%\Pointbreak\bin\pointbreak.exe")
# Example: C:\Users\Kevin\AppData\Local\Pointbreak\bin\pointbreak.exe
```

### VS Code Extension Binary

When you install the Pointbreak extension, it includes its own bundled binary for built-in agent support (GitHub Copilot, Cursor). This is separate from the CLI binary.

**Extension binary location** (for reference only):
- macOS: `~/.vscode/extensions/pointbreak.pointbreak-*/bin/`
- Windows: `%USERPROFILE%\.vscode\extensions\pointbreak.pointbreak-*\bin\`

**You should NOT use the extension binary for MCP configuration.** Always use the CLI binary installed via the install script.

---

## Common Issues

### "Command not found"

**Symptom**: AI assistant can't find `pointbreak` binary

**Solution**:
1. Verify installation: `pointbreak --version`
2. If not installed, run install script
3. Check config uses absolute path (not `pointbreak`, use `/full/path/to/pointbreak`)

### "Permission denied"

**Symptom**: Binary exists but can't execute

**Solution** (macOS / Linux):
```bash
chmod +x ~/.local/bin/pointbreak
```

### "Invalid JSON"

**Symptom**: Config file has syntax errors

**Solution**:
1. Validate JSON at [jsonlint.com](https://jsonlint.com)
2. Common errors:
   - Missing commas between entries
   - Trailing commas (not allowed in JSON)
   - Unescaped backslashes (Windows paths need `\\`)

**Windows path example**:
```json
✗ "command": "C:\Users\name\...\pointbreak.exe"  // Wrong - unescaped backslashes
✓ "command": "C:\\Users\\name\\...\\pointbreak.exe"  // Correct
```

### "MCP server not listed"

**Symptom**: AI assistant doesn't show Pointbreak in available servers

**Solution**:
1. Check config file exists and is valid JSON
2. Restart AI assistant after config changes
3. Check binary path is correct and points to existing file
4. Verify binary is executable (`ls -l` on Unix, check file properties on Windows)

---

## Testing Your Configuration

### 1. Verify Binary Installation

```bash
# Check binary exists and is executable
pointbreak --version

# Should output version number like:
# pointbreak 0.1.0
```

### 2. Verify Config File

```bash
# macOS / Linux
cat ~/.config/claude/claude_desktop_config.json

# Windows (PowerShell)
Get-Content "$env:APPDATA\Claude\claude_desktop_config.json"

# Should show valid JSON with pointbreak entry
```

### 3. Test with AI Assistant

Start your AI assistant and ask:
```
"List available MCP servers"
```

Expected response should include Pointbreak.

### 4. Test Debugging Features

Ask your AI assistant:
```
"Set a breakpoint at line 10 of main.py"
```

The breakpoint should appear in your IDE.

---

## Getting Help

If you're still having issues after checking this reference:

1. **Check logs**:
   - VS Code: View → Output → "Pointbreak MCP Server"
   - Look for connection errors or startup issues

2. **Verify prerequisites**:
   - Extension installed and enabled in VS Code
   - Debug adapter installed for your language
   - Project open in VS Code

3. **Documentation**:
   - [Installation Guide](installation.md)
   - [AI Assistants Guide](ai-assistants.md)
   - [Troubleshooting](troubleshooting.md)

4. **Community**:
   - [GitHub Discussions](https://github.com/withpointbreak/pointbreak/discussions)
   - [GitHub Issues](https://github.com/withpointbreak/pointbreak/issues)
