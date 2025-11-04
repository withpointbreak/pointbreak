# Troubleshooting Guide

Common issues and solutions for Pointbreak.

## AI Assistant Can't Find Pointbreak

### Symptoms

- AI responds: "I don't have access to debugging tools"
- AI tries to add console.log instead of setting breakpoints
- No debugger control

### Solutions

**1. Check Extension is Installed**

- Go to Extensions in VS Code
- Search for "Pointbreak"
- Verify it's installed and enabled

**2. Check Output Panel**

- View → Output
- Select "Pointbreak MCP Server" from dropdown
- Look for "Pointbreak MCP server started"

**3. Restart Editor**

- Close and reopen VS Code/Cursor
- Try again

**4. Check MCP Server Path** (for non-Claude-Code clients)

- Verify MCP configuration points to correct binary path
- Check file permissions

## Breakpoints Not Being Set

### Symptoms

- AI says it set a breakpoint, but no red dot appears
- Breakpoint appears but isn't hit during execution

### Solutions

**1. Verify Debug Adapter is Installed**

For your language, install the appropriate debug adapter:

- **Python:** Install official Python extension
- **JavaScript/TypeScript:** Built into VS Code
- **Rust/C/C++:** Install CodeLLDB extension
- **Go:** Install Go extension

**2. Check File Paths**

- Ensure you're referencing the correct file
- Use absolute paths or workspace-relative paths
- Check file actually exists

**3. Test Manual Debugging**

- Try setting a breakpoint manually (click in gutter)
- Press F5 to start debugging
- If manual debugging doesn't work, it's a configuration issue

**4. Check Launch Configuration**

- Open `.vscode/launch.json`
- Verify configuration for your language exists
- Test with VS Code's debugger first

## Debug Session Won't Start

### Symptoms

- AI tries to start debugging but nothing happens
- Error messages about debug adapter

### Solutions

**1. Check Debug Configuration Exists**

Create `.vscode/launch.json` if missing:

**Python:**

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "name": "Python: Current File",
      "type": "python",
      "request": "launch",
      "program": "${file}"
    }
  ]
}
```

**Rust:**

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "name": "Debug",
      "type": "lldb",
      "request": "launch",
      "program": "${workspaceFolder}/target/debug/your-program"
    }
  ]
}
```

**2. Test Manual Debugging**

- Press F5
- Select debug configuration
- If this fails, configuration is wrong

**3. Check Debug Adapter Logs**

- View → Output
- Select your debug adapter from dropdown
- Look for error messages

## AI Suggests Adding console.log

### Symptoms

- AI wants to modify code with print/console.log statements
- AI doesn't use breakpoints

### Solutions

**Be Explicit in Your Request:**

❌ "Debug this function"
✅ "Use the debugger to investigate this function. Set breakpoints and step through."

**Remind the AI:**

```
"Don't add console.log - use breakpoints and the debugger instead"
```

## Extension Errors in Output Panel

### "Failed to start MCP server"

**Solutions:**

1. Check Pointbreak binary exists (bundled with extension)
2. Check file permissions
3. Reinstall extension
4. Check antivirus isn't blocking

### "WebSocket connection failed"

**Solutions:**

1. Check no other process using the port
2. Restart VS Code
3. Check firewall settings (should allow localhost connections)

### "Debug adapter not found"

**Solutions:**

1. Install the debug adapter for your language
2. Restart VS Code after installation
3. Check extension is compatible with VS Code version

## Performance Issues

### Debugging is Slow

**Solutions:**

1. Reduce number of watch expressions
2. Use conditional breakpoints to limit pauses
3. Close unnecessary editor tabs
4. Restart VS Code. Debug adapters can leak memory over time and may have performance issues themselves.

### High CPU Usage

**Solutions:**

1. Check for runaway debug sessions
2. Restart Pointbreak using your AI assistant's MCP controls
3. Check debug adapter logs for issues

## Platform-Specific Issues

### macOS

**"Cannot open application" warning**

**Solution:**

1. Right-click Pointbreak binary
2. Select "Open"
3. Confirm you want to open it. Note that the binary will be officially signed and notarized.

### Linux

**Permission denied errors**

**Solution:**

```bash
# Make binary executable
chmod +x /path/to/pointbreak-binary
```

### Windows

**Windows Defender blocking**

**Solution:**

1. Add Pointbreak to Windows Defender exclusions
2. Path is usually: `%USERPROFILE%\.vscode\extensions\pointbreak.*`

## Still Having Issues?

### Collect Diagnostic Information

1. **Extension Version**

   - Check in Extensions panel

2. **VS Code Version**

   - Help → About

3. **Output Panel Logs**

   - View → Output
   - Select "Pointbreak MCP Server"
   - Copy all output

4. **Debug Adapter Logs**
   - View → Output
   - Select your debug adapter
   - Copy relevant errors

### Report the Issue

File a bug report: [GitHub Issues](https://github.com/withpointbreak/pointbreak/issues/new?template=bug_report.yml)

Include:

- Steps to reproduce
- Expected vs actual behavior
- Diagnostic information above
- Screenshots if applicable

### Ask the Community

Not sure if it's a bug? Ask in [GitHub Discussions](https://github.com/withpointbreak/pointbreak/discussions).

## Common Questions

**Q: Does Pointbreak work offline?**
A: Yes! Everything runs locally.

**Q: Do I need an internet connection?**
A: Only for downloading/installing. After that, fully offline.

**Q: Can I use Pointbreak with remote development?**
A: Yes, if your IDE supports remote debugging.

**Q: Does it work in SSH sessions?**
A: Yes, via VS Code Remote-SSH extension.

## More Help

- [Getting Started](getting-started.md)
- [Installation Guide](installation.md)
- [Usage Guide](usage.md)
- [FAQ](faq.md)
