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

Claude Code requires MCP server installation.

### Setup

1. Install Pointbreak extension in VS Code
2. Install MCP server:
   ```bash
   # macOS / Linux
   curl -fsSL https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.sh | sh

   # Windows (PowerShell)
   irm https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.ps1 | iex
   ```
3. Install Claude Code: [docs.claude.com](https://docs.claude.com)
4. Open your project in VS Code
5. Open integrated terminal (Cmd+` / Ctrl+`)
6. Start Claude Code: `claude-code`

Claude will discover Pointbreak through your MCP configuration.

### Usage

Just ask Claude to debug your code:

```
"Set a breakpoint at line 42 and start debugging"
"Step through this function and show me variable values"
"Why is this returning null?"
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

Codex can use Pointbreak through MCP. Requires MCP server installation.

### Setup

1. Install Pointbreak extension in VS Code
2. Install MCP server:
   ```bash
   # macOS / Linux
   curl -fsSL https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.sh | sh

   # Windows (PowerShell)
   irm https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.ps1 | iex
   ```
3. Configure Codex's MCP settings to point to the installed binary

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

Any MCP-compatible client can use Pointbreak.

### Configuration

Add to your MCP client configuration:

```json
{
  "mcpServers": {
    "pointbreak": {
      "command": "/path/to/pointbreak-binary"
    }
  }
}
```

The binary path depends on your platform:
- macOS/Linux: Usually in extension folder
- Windows: Check extension installation directory

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
