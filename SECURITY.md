# Security Policy

## Reporting Security Vulnerabilities

The security of Pointbreak is a top priority. If you discover a security vulnerability, please follow responsible disclosure practices.

### ⚠️ Do NOT Create Public Issues

**Do not file public GitHub issues for security vulnerabilities.** Public disclosure of security issues can put users at risk before a fix is available.

### How to Report

To report a security vulnerability, please email:

**security@withpointbreak.com**

Include the following information in your report:

1. **Description** - Clear description of the vulnerability
2. **Impact** - What could an attacker accomplish?
3. **Steps to Reproduce** - Detailed steps to reproduce the issue
4. **Proof of Concept** - Code, screenshots, or videos demonstrating the vulnerability (if applicable)
5. **Affected Versions** - Which version(s) of Pointbreak are affected
6. **Suggested Fix** - If you have ideas for how to fix it (optional)

### What to Expect

After you submit a security report:

1. **Acknowledgment** - We'll acknowledge receipt within 2 business days
2. **Assessment** - We'll investigate and assess severity within 5 business days
3. **Updates** - We'll keep you informed of progress
4. **Fix** - We'll develop and test a fix
5. **Release** - We'll release a patched version
6. **Credit** - We'll credit you in the release notes (unless you prefer to remain anonymous)

### Security Update Policy

- **Critical vulnerabilities:** Patched and released as soon as possible (typically within 7 days)
- **High severity:** Patched in the next release (typically within 30 days)
- **Medium/Low severity:** Patched in regular releases

### Scope

Security reports are in scope for:

- **Pointbreak VS Code Extension** - Vulnerabilities in the extension code
- **Pointbreak MCP Server** - Vulnerabilities in the MCP server binary
- **Privilege Escalation** - Local privilege escalation
- **Code Execution** - Remote or local code execution
- **Information Disclosure** - Exposure of sensitive information
- **Debugger Control** - Unauthorized debugger access or control

### Out of Scope

The following are out of scope:

- Third-party dependencies (report directly to the maintainer)
- VS Code itself (report to Microsoft)
- Debug adapters (CodeLLDB, debugpy, etc. - report to their maintainers)
- Social engineering attacks
- Physical attacks
- Denial of service via resource exhaustion
- Issues requiring physical access to the machine

### Safe Harbor

We support safe harbor for security researchers who:

- Make a good faith effort to avoid privacy violations, data destruction, and service interruption
- Only interact with accounts you own or with explicit permission of the account holder
- Do not exploit a vulnerability beyond what is necessary to demonstrate it
- Report vulnerabilities promptly
- Keep vulnerability details confidential until a fix is released

## Security Best Practices for Users

### Keep Pointbreak Updated

Always use the latest version of Pointbreak. Security fixes are included in new releases.

- **VS Code Extension:** Enable auto-updates in VS Code settings
- **Manual Installation:** Check [GitHub Releases](https://github.com/withpointbreak/pointbreak/releases) regularly

### Use Trusted Debug Adapters

Only use debug adapters from trusted sources:
- Official debug adapters (CodeLLDB, debugpy, Node Debug)
- Well-maintained community adapters with good security track records

### Be Cautious with Debug Configurations

- Review launch configurations before debugging
- Be careful with debug configurations from untrusted sources
- Don't run debug sessions on untrusted code

### Report Suspicious Behavior

If you notice suspicious behavior while using Pointbreak:
- Stop using the extension immediately
- Document what you observed
- Report to **security@withpointbreak.com**

## Privacy & Data Collection

Pointbreak does not:
- Collect telemetry
- Send code to external servers
- Phone home for any reason
- Track usage or analytics

All debugging happens locally on your machine. MCP communication is local-only (via terminal or WebSocket on localhost).

## Known Security Considerations

### Debug Adapter Access

Pointbreak provides programmatic access to your IDE's debugger. AI assistants with access to Pointbreak can:
- Set breakpoints in your code
- Read variable values during debugging
- Step through code execution
- Evaluate expressions in the debug context

**Recommendation:** Only use trusted AI assistants and MCP clients with Pointbreak.

### WebSocket Bridge

The Pointbreak bridge exposes a local WebSocket server on localhost. This is intentionally limited to local connections only.

**Recommendation:** Do not expose the bridge to external networks.

## Questions?

For security-related questions that don't involve vulnerabilities, you can:
- Ask in [GitHub Discussions](https://github.com/withpointbreak/pointbreak/discussions)
- Review documentation at [withpointbreak.com](https://withpointbreak.com)

For vulnerability reports, always email **security@withpointbreak.com**.

---

Thank you for helping keep Pointbreak and its users safe!
