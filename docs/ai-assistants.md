# AI Assistants Guide

Setup guides for using Pointbreak with different AI assistants.

## Two Installation Paths

**Built-in AI agents (GitHub Copilot, Cursor):**
- Just install the extension—MCP server auto-registers
- No additional configuration needed

**External AI assistants (Claude Code, Codex, Windsurf, etc.):**
- Install the extension + install MCP server separately
- Configure your AI assistant to use the MCP server

---

## GitHub Copilot (VS Code Built-in)

GitHub Copilot works automatically with Pointbreak—no additional setup needed!

### Setup

1. Install Pointbreak extension in VS Code
2. Use GitHub Copilot as normal

The MCP server auto-registers with GitHub Copilot when you install the extension.

### Usage

Ask Copilot to debug your code:

```
"Set a breakpoint at line 42 and start debugging"
"Step through this function and show me variable values"
"Why is this returning null?"
```

## Claude Code

Claude Code requires MCP server installation and configuration.

### Setup

**Step 1: Install Prerequisites**

1. Install Pointbreak extension in VS Code
2. Install Pointbreak CLI:
   ```bash
   # macOS / Linux
   curl -fsSL https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.sh | sh

   # Windows (PowerShell)
   irm https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.ps1 | iex
   ```

**Step 2: Add Pointbreak as an MCP Server**

Use the Claude Code CLI to add Pointbreak:

```bash
claude mcp add --transport stdio pointbreak -- pointbreak
```

This command:
- Registers Pointbreak as an MCP server in Claude Code
- Uses the `pointbreak` binary from your PATH (installed in Step 1)
- Configures it to communicate via stdio transport

**Step 3: Verify Installation**

Start Claude Code and use the `/mcp` command to view configured servers. You should see Pointbreak listed.

### Usage

Ask Claude to debug your code:

```
"Set a breakpoint at line 42 and start debugging"
"Step through this function and show me variable values"
"Why is this returning null?"
```

### Troubleshooting

**Claude can't find Pointbreak MCP server**:
- Verify the CLI is installed: `pointbreak --version`
- Check PATH includes binary location: `which pointbreak` (macOS/Linux) or `where.exe pointbreak` (Windows)
- List configured servers: `claude mcp list`
- Try removing and re-adding: `claude mcp remove pointbreak` then re-add

**Pointbreak binary not found**:
- Make sure you completed Step 1 (install script)
- Verify installation: `pointbreak --version`
- If using custom path, specify it explicitly:
  ```bash
  claude mcp add --transport stdio pointbreak -- /full/path/to/pointbreak
  ```

## Cursor (Built-in Agent)

Cursor's built-in AI agent works automatically with Pointbreak—no additional setup needed!

### Setup

1. Install Pointbreak extension in Cursor
2. Use Cursor's AI panel (Cmd+K / Ctrl+K)

The MCP server auto-registers with Cursor's built-in agent when you install the extension.

### Usage

Ask Cursor's AI to debug:

```
"Debug this test and tell me why it fails"
"Set a breakpoint and inspect the state"
```

## Codex

Codex can use Pointbreak through MCP.

### Setup

**Step 1: Install Prerequisites**

1. Install Pointbreak extension in VS Code
2. Install Pointbreak CLI:
   ```bash
   # macOS / Linux
   curl -fsSL https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.sh | sh

   # Windows (PowerShell)
   irm https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.ps1 | iex
   ```

**Step 2: Add Pointbreak as an MCP Server**

Use the Codex CLI to add Pointbreak:

```bash
codex mcp add pointbreak -- pointbreak
```

**Step 3: Verify Installation**

Start Codex and use the `/mcp` command to view configured servers. You should see Pointbreak listed.

### Usage

Ask Codex to use debugging tools through natural language.

## Windsurf

Windsurf requires manual MCP server installation.

### Setup

1. Install Pointbreak extension in Windsurf
2. Install MCP server:
   ```bash
   # macOS / Linux
   curl -fsSL https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.sh | sh

   # Windows (PowerShell)
   irm https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.ps1 | iex
   ```
3. Configure Windsurf's MCP settings to point to the installed binary

## Other MCP Clients

Any MCP-compatible client can use Pointbreak through the Model Context Protocol.

### Setup

1. Install Pointbreak extension in your IDE
2. Install Pointbreak CLI:
   ```bash
   # macOS / Linux
   curl -fsSL https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.sh | sh

   # Windows (PowerShell)
   irm https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.ps1 | iex
   ```
3. Configure your MCP client to use the Pointbreak binary

Refer to your MCP client's documentation for specific configuration instructions. The Pointbreak binary is typically installed at:
- **macOS/Linux**: `~/.local/bin/pointbreak`
- **Windows**: `%LOCALAPPDATA%\Pointbreak\bin\pointbreak.exe`

## Tips for All AI Assistants

### Be Specific

❌ "Debug this"
✅ "Set a breakpoint on line 42 of main.py and start debugging"

### Set Breakpoints First

❌ "Start debugging and figure out the bug"
✅ "Set a breakpoint at the error, then start debugging"

### Ask for Context

- "Show me the stack trace"
- "What are the local variables?"
- "Evaluate this expression"

### Use Natural Language

You don't need special syntax—just describe what you want!

## Troubleshooting

### AI Can't Find Pointbreak

**Check:**
1. Extension is installed and enabled
2. Output panel shows "Pointbreak MCP server started"
3. Restart your editor

### AI Uses console.log Instead

**Solution:** Be explicit:

```
"Use the debugger to investigate this, don't add console.log"
"Set a breakpoint and step through"
```

### Breakpoints Don't Work

**Check:**
1. Debug adapter is installed for your language
2. Debug configuration exists
3. Try manual debugging (F5) first

## More Information

- [Getting Started Guide](getting-started.md)
- [Usage Examples](usage.md)
- [Troubleshooting](troubleshooting.md)
