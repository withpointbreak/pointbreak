# AI Assistants Guide

Setup guides for using Pointbreak with different AI assistants.

## Claude Code (Recommended)

Claude Code works automatically with Pointbreak—no configuration needed!

### Setup

1. Install Pointbreak extension in VS Code
2. Install Claude Code: [docs.claude.com](https://docs.claude.com)
3. Open your project in VS Code
4. Open integrated terminal (Cmd+` / Ctrl+`)
5. Start Claude Code: `claude-code`

That's it! Claude automatically discovers and connects to Pointbreak.

### Usage

Just ask Claude to debug your code:

```
"Set a breakpoint at line 42 and start debugging"
"Step through this function and show me variable values"
"Why is this returning null?"
```

## Cursor

Cursor has built-in MCP support and Pointbreak auto-registers.

### Setup

1. Install Pointbreak extension in Cursor
2. Use Cursor's AI panel (Cmd+K / Ctrl+K)

Pointbreak is automatically available as an MCP server.

### Usage

Ask Cursor's AI to debug:

```
"Debug this test and tell me why it fails"
"Set a breakpoint and inspect the state"
```

## Cline (VS Code Extension)

Cline can use Pointbreak through MCP.

### Setup

1. Install Pointbreak extension
2. Install Cline extension
3. Configure Cline to use MCP servers

### Usage

Ask Cline to use debugging tools through natural language.

## Zed

Zed supports MCP servers.

### Setup

Configure Pointbreak in Zed's MCP settings:

```json
{
  "mcp_servers": {
    "pointbreak": {
      "command": "/path/to/pointbreak-binary"
    }
  }
}
```

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
