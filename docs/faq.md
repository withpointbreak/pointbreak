# Frequently Asked Questions

## General

### What is Pointbreak?

Pointbreak enables AI assistants to control your VS Code debugger through natural language. Instead of adding print statements, AI can set breakpoints, step through code, and inspect variables.

### Is Pointbreak free?

Yes! Pointbreak is free to use. The binaries and VS Code extension are freely available, though the source code is proprietary.

### Does Pointbreak work offline?

Yes! All debugging happens locally on your machine. No internet connection required after installation.

### What languages does it support?

Pointbreak works with any language your IDE can debug:
- Rust, C, C++ (via CodeLLDB)
- Python (via debugpy)
- JavaScript, TypeScript (via Node Debug)
- Go (via Delve)
- Any language with a Debug Adapter Protocol implementation

## Technical

### How does it work?

Pointbreak bridges AI assistants to your IDE's native debugger using the Model Context Protocol (MCP). It doesn't implement a new debugger—it uses your existing debug adapters.

See [Architecture](architecture.md) for details.

### Is my code sent anywhere?

No. All debugging happens locally. Pointbreak does not:
- Send code to external servers
- Collect telemetry
- Phone home
- Track usage

### Does it modify my code?

No. Pointbreak only:
- Sets breakpoints (temporary, not saved to files)
- Inspects variable values
- Controls execution flow

Your source files are never modified.

### Can I use it without an AI assistant?

Technically yes, but it's designed for AI interaction. For manual debugging, use your IDE's built-in debugger.

## Installation & Setup

### Which AI assistants work with Pointbreak?

Any MCP-compatible AI assistant:
- GitHub Copilot (VS Code built-in) ✅
- Cursor (built-in agent) ✅
- Claude Code ✅
- Cline ✅
- Custom MCP clients ✅

### Do I need to configure anything?

**GitHub Copilot / Cursor (built-in agents):** No configuration needed—MCP server auto-registers when you install the extension.

**Claude Code, Cline, Windsurf, and other external AI assistants:** Need to install the MCP server separately and configure your AI assistant.

See [AI Assistants Guide](ai-assistants.md).

### What if my language isn't listed?

If your language has a Debug Adapter Protocol (DAP) debugger for VS Code, Pointbreak will work with it.

## Usage

### Why doesn't the AI use breakpoints?

The AI might not realize debugging tools are available. Try being explicit:

```
"Use the debugger to investigate this. Set breakpoints and step through."
```

### Can I set breakpoints manually and have AI use them?

Yes! Manual breakpoints and AI-set breakpoints work together.

### Do breakpoints persist between sessions?

Breakpoints set by the AI are temporary (per debug session). Manual breakpoints in your IDE persist as usual.

### Can multiple people debug the same code?

Each person has their own local debug session. Shared debugging is not currently supported.

## Troubleshooting

### The AI can't find Pointbreak

**Solutions:**
1. Check extension is installed and enabled
2. Restart your editor
3. Check Output panel for errors

See [Troubleshooting Guide](troubleshooting.md).

### Breakpoints aren't working

**Common causes:**
1. Debug adapter not installed for your language
2. No debug configuration (`.vscode/launch.json`)
3. File path issues

See [Troubleshooting Guide](troubleshooting.md).

### Debugging is slow

**Try:**
1. Reduce watch expressions
2. Use conditional breakpoints
3. Restart VS Code
4. Check debug adapter logs

## Privacy & Security

### What data does Pointbreak collect?

None. Pointbreak does not collect any telemetry, analytics, or usage data.

### Is it safe to use at work?

Yes. Pointbreak:
- Runs entirely locally
- Doesn't send data externally
- Doesn't modify your code
- Respects your IDE's security model

### Can I use it on proprietary code?

Yes. Your code never leaves your machine.

### How do I report security issues?

Email: security@withpointbreak.com

Do not file public issues for security vulnerabilities.

See [SECURITY.md](../SECURITY.md).

## Licensing & Source Code

### Is Pointbreak open source?

No. Pointbreak is proprietary software. The source code is not publicly available.

### Can I contribute code?

We do not accept code contributions, but we welcome:
- Bug reports
- Feature requests
- Documentation improvements
- Community discussions

See [CONTRIBUTING.md](../CONTRIBUTING.md).

### Why isn't it open source?

We've chosen to launch as proprietary software. We may open source in the future based on community demand.

### Can I see the source code?

No. The source code is proprietary and not available for viewing.

## Platform Support

### What operating systems are supported?

- macOS 10.15+ (x64 and ARM64)
- Windows 10+ (x64 and ARM64)
- Linux (recent distributions, x64 and ARM64)

### What about VS Code alternatives?

Pointbreak works with VS Code-compatible editors that support extensions:
- Cursor ✅
- Windsurf ✅
- VS Codium ✅
- Others with extension support ✅

### Does it work in remote development?

Yes, if your IDE supports remote debugging (e.g., VS Code Remote-SSH).

## Future Plans

### Will there be a paid version?

No current plans. Pointbreak is free.

### What features are coming next?

We're considering:
- Replay debugging
- Team collaboration features
- Smart breakpoint suggestions
- Debug analytics

See our [GitHub Discussions](https://github.com/withpointbreak/pointbreak/discussions) for roadmap input.

### Will it become open source?

Maybe! We'll open source if there's strong community demand.

## Getting Help

### Where do I ask questions?

- **General questions:** [GitHub Discussions](https://github.com/withpointbreak/pointbreak/discussions)
- **Bug reports:** [GitHub Issues](https://github.com/withpointbreak/pointbreak/issues)
- **Security issues:** security@withpointbreak.com

### How do I report a bug?

File an issue: [GitHub Issues](https://github.com/withpointbreak/pointbreak/issues/new?template=bug_report.yml)

Include:
- Steps to reproduce
- Expected vs actual behavior
- Logs from Output panel
- Screenshots if applicable

### How do I request a feature?

File a feature request: [GitHub Issues](https://github.com/withpointbreak/pointbreak/issues/new?template=feature_request.yml)

Describe:
- The problem you're solving
- How you envision the feature
- Use cases and examples

## More Information

- [Getting Started](getting-started.md)
- [Installation Guide](installation.md)
- [Usage Guide](usage.md)
- [Troubleshooting](troubleshooting.md)
- [Website](https://withpointbreak.com)
