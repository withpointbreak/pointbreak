# Getting Started with Pointbreak

This guide will help you set up Pointbreak and complete your first AI-assisted debugging session.

## Prerequisites

Before you begin, you'll need:

- **VS Code** (or compatible editor: Cursor, Windsurf, VS Codium)
- **An MCP-compatible AI assistant** (Claude Code, Cursor AI, Codex, etc.)
- **A debug adapter** for your language (e.g., CodeLLDB for Rust, debugpy for Python)

## Step 1: Install the Pointbreak Extension

### Via VS Code Marketplace (Recommended)

1. Open VS Code
2. Go to Extensions (Cmd+Shift+X / Ctrl+Shift+X)
3. Search for "Pointbreak"
4. Look for publisher: `pointbreak`
5. Click "Install"

### Via Manual VSIX

1. Download the appropriate VSIX from [GitHub Releases](https://github.com/withpointbreak/pointbreak/releases)
2. Run: `code --install-extension pointbreak-*.vsix`

## Step 2: Install a Debug Adapter

Pointbreak works with your IDE's native debugger, so you need the appropriate debug adapter for your language.

### Rust (CodeLLDB)

1. Open VS Code Extensions
2. Search for "CodeLLDB"
3. Install the extension by vadimcn

### Python (debugpy)

1. Open VS Code Extensions
2. Search for "Python"
3. Install the official Python extension by Microsoft (includes debugpy)

### JavaScript/TypeScript (Node Debug)

Built into VS Code - no installation needed!

### Go (Delve)

1. Open VS Code Extensions
2. Search for "Go"
3. Install the official Go extension (includes Delve integration)

## Step 3: Set Up Your AI Assistant

### Option A: Claude Code (Recommended)

1. Install Claude Code: [docs.claude.com](https://docs.claude.com)
2. Open your project in VS Code
3. Open the integrated terminal (Cmd+` / Ctrl+`)
4. Start Claude Code in the terminal

**That's it!** Claude Code will automatically discover and connect to Pointbreak.

### Option B: Cursor

1. Install Pointbreak extension in Cursor
2. Use Cursor's AI panel

Pointbreak is automatically registered as an MCP server in Cursor.

### Option C: Other MCP Clients

Configure your MCP client to use Pointbreak. See [AI Assistants Guide](ai-assistants.md) for details.

## Step 4: Verify Installation

Check that Pointbreak is working:

1. Open the Output panel: View → Output
2. Select "Pointbreak MCP Server" from the dropdown
3. You should see: `Pointbreak MCP server started`

If you see errors, check [Troubleshooting](troubleshooting.md).

## Step 5: Your First Debugging Session

Let's debug a simple program!

### Example Code (Python)

Create `test.py`:

```python
def calculate_average(numbers):
    total = sum(numbers)
    count = len(numbers)
    return total / count

result = calculate_average([1, 2, 3, 4, 5])
print(f"Average: {result}")
```

### Ask Your AI Assistant

Try this prompt:

```
"Set a breakpoint on line 3 of test.py and start debugging.
Show me the value of 'numbers' when we hit the breakpoint."
```

### What Should Happen

1. **Breakpoint appears** - You'll see a red dot on line 3 in VS Code
2. **Debug session starts** - The debugger launches automatically
3. **Execution pauses** - Code stops at line 3
4. **AI inspects variables** - The AI shows you the value of `numbers`

### Example AI Response

```
I've set a breakpoint at line 3 and started debugging.
The execution has paused at the breakpoint.

The value of 'numbers' is: [1, 2, 3, 4, 5]

Would you like me to step through the rest of the function?
```

## Common First-Time Issues

### "AI can't find Pointbreak"

**Solution:** Make sure the extension is installed and enabled. Restart VS Code if needed.

### "Breakpoint not hit"

**Solution:** Ensure your debug configuration is correct. Try running the debugger manually first (F5) to verify it works.

### "No debug adapter available"

**Solution:** Install the debug adapter for your language (see Step 2).

## Next Steps

Now that you're up and running:

- Read the [Usage Guide](usage.md) for more debugging workflows
- Check out [AI Assistants](ai-assistants.md) for setup guides
- Explore [common debugging patterns](usage.md#common-patterns)
- Join [GitHub Discussions](https://github.com/withpointbreak/pointbreak/discussions) to share your experience

## Tips for Effective AI Debugging

1. **Be specific** - "Set a breakpoint on line 42" is clearer than "debug this"
2. **Set breakpoints first** - Ask the AI to set breakpoints before starting the debugger
3. **Ask for context** - "Show me the stack trace" or "What are all the local variables?"
4. **Step through interactively** - "Step into this function" or "Step over the next line"
5. **Use natural language** - You don't need to memorize commands!

## Questions?

- **Troubleshooting:** See [Troubleshooting Guide](troubleshooting.md)
- **More examples:** See [Usage Guide](usage.md)
- **Ask the community:** [GitHub Discussions](https://github.com/withpointbreak/pointbreak/discussions)

---

**Ready to debug?** Try asking your AI assistant to help you debug your code!
