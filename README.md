# Pointbreak

**AI assistants can write code. Now they can debug it too.**

> **Note:** This is the public documentation and issue tracking repository for Pointbreak. The source code is proprietary. For documentation, downloads, and support, you're in the right place!

## What is Pointbreak?

Pointbreak enables AI assistants to control VS Code debuggers through natural language. Set breakpoints, step through code, and inspect variables—all through AI.

Your AI assistant can now:
- 🎯 **Set breakpoints** through natural language ("break on line 42")
- 🔍 **Inspect variables** while your code runs ("show me user_input")
- 🪜 **Step through execution** ("step into this function")
- 🐛 **Find bugs** by actually running and examining your code

Works with GitHub Copilot, Cursor, Claude Code, Codex, and other MCP-compatible AI assistants.

## Quick Start

### For GitHub Copilot / Cursor Users

1. **Install** the Pointbreak extension
   - Search "Pointbreak" in Extensions (publisher: `pointbreak`)
   - [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=pointbreak.pointbreak)

2. **Ask** your AI assistant to debug your code
   ```
   "Set a breakpoint on main.rs line 42 and start debugging"
   ```

That's it. The MCP server auto-registers with your built-in AI agent.

### For Other AI Assistants (Claude Code, Codex, Windsurf, etc.)

1. **Install** the Pointbreak extension (same as above)

2. **Install** the MCP server on your system:
   ```bash
   # macOS / Linux
   curl -fsSL https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.sh | sh

   # Windows (PowerShell)
   irm https://raw.githubusercontent.com/withpointbreak/pointbreak/main/scripts/install.ps1 | iex
   ```

3. **Configure** your AI assistant's MCP settings
   - See detailed setup guides: [docs/ai-assistants.md](docs/ai-assistants.md)

## Downloads

### VS Code Extension (Recommended)

Install directly from the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=pointbreak.pointbreak).

Works in: VS Code, Cursor, Windsurf, VS Codium, and other VS Code-compatible editors.

### Platform-Specific Extensions

Download manual VSIX packages from [GitHub Releases](https://github.com/withpointbreak/pointbreak/releases):

| Platform | Download |
|----------|----------|
| macOS (Apple Silicon) | `pointbreak-darwin-arm64-*.vsix` |
| macOS (Intel) | `pointbreak-darwin-x64-*.vsix` |
| Linux x64 | `pointbreak-linux-x64-*.vsix` |
| Linux ARM64 | `pointbreak-linux-arm64-*.vsix` |
| Windows x64 | `pointbreak-win32-x64-*.vsix` |
| Windows ARM64 | `pointbreak-win32-arm64-*.vsix` |

Install: `code --install-extension pointbreak-*.vsix`

### Standalone Binaries

For advanced users, standalone MCP server binaries are available from [GitHub Releases](https://github.com/withpointbreak/pointbreak/releases).

## Documentation

- **[Getting Started](docs/getting-started.md)** - Step-by-step guide to your first debugging session
- **[Installation](docs/installation.md)** - Detailed installation instructions for all platforms
- **[AI Assistants](docs/ai-assistants.md)** - Setup guides for Claude Code, Cursor, and more
- **[Usage Guide](docs/usage.md)** - Examples and common debugging workflows
- **[Troubleshooting](docs/troubleshooting.md)** - Common issues and solutions
- **[Architecture](docs/architecture.md)** - High-level architecture overview
- **[FAQ](docs/faq.md)** - Frequently asked questions

## Supported Platforms

**Languages** (anything your IDE can debug):
- Rust, C, C++ (via CodeLLDB)
- Python (via debugpy)
- JavaScript, TypeScript (via Node Debug / VS Code JS Debug)
- Go (via Delve)
- Any language with a Debug Adapter Protocol implementation

**AI Assistants** (MCP-compatible):
- GitHub Copilot (VS Code built-in)
- Cursor (built-in agent)
- Claude Code
- Codex
- Any tool supporting Model Context Protocol

**Operating Systems:**
- macOS (x64 + ARM64)
- Linux (x64 + ARM64)
- Windows (x64 + ARM64)

## Example

```
User: "Debug this test and tell me why user_input is empty"

AI: Setting breakpoint at line 15... Starting debugger...
    [Breakpoint appears in VS Code]
    [Debug session starts]
    [Code pauses at breakpoint]

    Found it! You're reading user_input before prompting the user.
    The input happens on line 18, but you're using it on line 15.
    Move the prompt above the read.
```

## How It Works

Pointbreak bridges AI assistants to your IDE's **native debugger**:

```
┌─────────────┐      MCP          ┌──────────────┐      VS Code API      ┌─────────────┐
│     AI      │  ─── Protocol ──► │  Pointbreak  │  ─────────────────►   │   Native    │
│  Assistant  │                   │  Extension   │                       │  Debugger   │
└─────────────┘                   └──────────────┘                       └─────────────┘
```

**Key insight:** Instead of building a new debugger, Pointbreak uses your IDE's existing debugger. You get all your installed debug adapters, breakpoint UI, and variable inspection—but now controllable through AI.

## Contributing

Pointbreak is free, but it's not currently Open Source Software. At this early stage, your feedback is highly valued in helping shape the future of the project:

- 🐛 **[Bug reports](https://github.com/withpointbreak/pointbreak/issues/new?template=bug_report.yml)** - Help identify and fix issues
- 💡 **[Feature requests](https://github.com/withpointbreak/pointbreak/issues/new?template=feature_request.yml)** - Share your ideas for improvements
- 💬 **[Discussions](https://github.com/withpointbreak/pointbreak/discussions)** - Share your use cases and experiences
- 📝 **Documentation improvements** - Suggest clearer explanations

**Note:** We do not accept code contributions at this time. See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## Support

- 🐛 **Issues:** [GitHub Issues](https://github.com/withpointbreak/pointbreak/issues)
- 💬 **Discussions:** [GitHub Discussions](https://github.com/withpointbreak/pointbreak/discussions)
- 🌐 **Website:** [withpointbreak.com](https://withpointbreak.com)
- 🔒 **Security:** See [SECURITY.md](SECURITY.md) for reporting security vulnerabilities

## License

Proprietary License - Copyright (c) 2025 Kevin Swiber. All rights reserved.

**Pointbreak is free to use** (free binaries and VS Code Marketplace extension), but the source code is proprietary and not open source. See [LICENSE](LICENSE) for details.

---

**Made with ❤️ for developers who want AI that can actually debug their code.**
